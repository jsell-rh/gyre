<script>
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Tabs from '../lib/Tabs.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import { shortId } from '../lib/entityNames.svelte.js';
  import { NOTIF_TYPE_IDS } from '../lib/notification-types.js';
  import { getContext, untrack } from 'svelte';

  const navigate = getContext('navigate');
  const goToWorkspaceHome = getContext('goToWorkspaceHome');
  const goToUserProfile = getContext('goToUserProfile');

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

  // Notification preferences — per-type toggles + delivery channels
  // (HSI §12 + user-management.md §Delivery Channels). Both are stored
  // server-side and read/written through GET/PUT
  // /api/v1/notifications/preferences (task-112/125): the response is
  // `{preferences: [{notification_type, enabled}], channels: {...}}`.
  // NOTIF_TYPE_IDS (lib/notification-types.js) is the canonical
  // `NotificationType::as_str()` name list the backend stores in
  // user_notification_preferences.notification_type; PUT rejects any other
  // spelling, and the inbox filter only matches these names, so legacy parse
  // aliases like 'SpecPendingApproval'/'BudgetWarning' must not appear there.
  // scripts/check-notification-toggle-coverage.sh keeps it in lockstep with
  // gyre-common's NotificationType enum and the locale labels.
  // Build NOTIF_TYPES array with localized labels for compatibility with defaultPrefs()
  const NOTIF_TYPES = NOTIF_TYPE_IDS.map(id => ({ id, labelKey: `user_profile.notif_types.${id}` }));

  function defaultPrefs() {
    return Object.fromEntries(NOTIF_TYPES.map(t => [t.id, true]));
  }

  let notifPrefs = $state(defaultPrefs());
  let prefsSaving = $state(false);
  let prefsLoaded = $state(false);

  // Delivery channels (user-management.md §Delivery Channels): in_app is
  // always on (server rejects in_app=false); email carries a digest
  // frequency and a minimum priority band; webhook/slack are optional.
  const PRIORITIES = ['Low', 'Medium', 'High', 'Urgent'];
  const DIGESTS = ['Immediate', 'Hourly', 'Daily', 'Off'];
  let channels = $state({
    in_app: true,
    email: { enabled: false, digest: 'Off', min_priority: 'Medium' },
    webhook: null,
    slack: null,
  });
  let webhookDraft = $state({ url: '', secret: '', min_priority: 'Medium' });
  let slackDraft = $state({ webhook_url: '', channel: '', min_priority: 'Low' });

  function normalizeChannels(raw) {
    const email = raw?.email ?? {};
    return {
      in_app: true,
      email: {
        enabled: email.enabled ?? false,
        digest: email.digest ?? 'Off',
        min_priority: email.min_priority ?? 'Medium',
      },
      webhook: raw?.webhook ?? null,
      slack: raw?.slack ?? null,
    };
  }

  async function loadPrefs() {
    try {
      // GET /users/me/notification-preferences →
      //   { preferences: [{notification_type, enabled}], channels: {...} }
      const raw = await api.getNotificationPreferences();
      if (raw && typeof raw === 'object') {
        // Per-type preferences: array of {notification_type, enabled} with
        // canonical NotificationType names only.
        const items = Array.isArray(raw.preferences) ? raw.preferences : [];
        const next = defaultPrefs();
        for (const item of items) {
          if (typeof item?.notification_type === 'string' && typeof item.enabled === 'boolean') {
            next[item.notification_type] = item.enabled;
          }
        }
        notifPrefs = next;
        channels = normalizeChannels(raw.channels);
        webhookDraft = channels.webhook
          ? { url: channels.webhook.url ?? '', secret: '', min_priority: channels.webhook.min_priority ?? 'Medium' }
          : { url: '', secret: '', min_priority: 'Medium' };
        slackDraft = channels.slack
          ? { webhook_url: channels.slack.webhook_url ?? '', channel: channels.slack.channel ?? '', min_priority: channels.slack.min_priority ?? 'Low' }
          : { webhook_url: '', channel: '', min_priority: 'Low' };
      } else {
        notifPrefs = defaultPrefs();
      }
    } catch {
      // Server unreachable — fall back to defaults
      notifPrefs = defaultPrefs();
    }
    prefsLoaded = true;
  }

  function buildChannelsPayload() {
    const payload = {
      in_app: true,
      email: {
        enabled: channels.email.enabled,
        digest: channels.email.digest,
        min_priority: channels.email.min_priority,
      },
      webhook: null,
      slack: null,
    };
    if (webhookDraft.url.trim()) {
      payload.webhook = {
        url: webhookDraft.url.trim(),
        secret: webhookDraft.secret,
        min_priority: webhookDraft.min_priority,
      };
    }
    if (slackDraft.webhook_url.trim()) {
      payload.slack = {
        webhook_url: slackDraft.webhook_url.trim(),
        channel: slackDraft.channel.trim() || null,
        min_priority: slackDraft.min_priority,
      };
    }
    return payload;
  }

  async function savePrefs() {
    prefsSaving = true;
    try {
      // PUT /users/me/notification-preferences expects
      //   { preferences: [{notification_type, enabled}], channels: {...} }
      // with canonical NotificationType names (400 otherwise).
      await api.updateNotificationPreferences({
        preferences: NOTIF_TYPES.map(t => ({
          notification_type: t.id,
          enabled: notifPrefs[t.id] ?? true,
        })),
        channels: buildChannelsPayload(),
      });
      showToast($t('user_profile.prefs_saved'), { type: 'success' });
    } catch (e) {
      showToast($t('user_profile.prefs_failed') + (e?.message ? ': ' + e.message : ''), { type: 'error' });
    }
    prefsSaving = false;
  }

  // ── User preferences (user-management.md §User Preferences) ──────
  // Theme, UI density, diff view, feed scope, code font size — persisted
  // server-side via PUT /users/me `preferences` (task-120 partial update).
  const THEMES = ['Light', 'Dark', 'System'];
  const DENSITIES = ['Compact', 'Comfortable', 'Spacious'];
  const DIFF_VIEWS = ['SideBySide', 'Unified'];
  const FEED_SCOPES = ['MyActivity', 'Workspace', 'All'];
  let prefsForm = $state({ theme: 'System', ui_density: 'Comfortable', diff_view: 'SideBySide', activity_feed_scope: 'MyActivity', code_font_size: 14 });
  let prefsFormSaving = $state(false);

  function loadPrefsForm() {
    const p = me?.preferences ?? {};
    prefsForm = {
      theme: p.theme ?? 'System',
      ui_density: p.ui_density ?? 'Comfortable',
      diff_view: p.diff_view ?? 'SideBySide',
      activity_feed_scope: p.activity_feed_scope ?? 'MyActivity',
      code_font_size: p.code_font_size ?? 14,
    };
  }

  async function savePrefsForm() {
    prefsFormSaving = true;
    try {
      me = await api.updateMe({ preferences: {
        theme: prefsForm.theme,
        ui_density: prefsForm.ui_density,
        diff_view: prefsForm.diff_view,
        activity_feed_scope: prefsForm.activity_feed_scope,
        code_font_size: Number(prefsForm.code_font_size) || 14,
      } });
      showToast($t('user_profile.profile_updated'), { type: 'success' });
    } catch (e) {
      showToast('Failed to save preferences: ' + (e?.message ?? e), { type: 'error' });
    } finally {
      prefsFormSaving = false;
    }
  }

  // ── Pending workspace invitations (user-management.md §UI Pages >
  // Pending Invitation Accept Page) — my pending WorkspaceInvitation
  // rows, with accept / decline through the task-110 invitee endpoints.
  let pendingInvitations = $state([]);
  let invitationsLoading = $state(false);
  let invitationBusyId = $state(null);

  async function loadInvitations() {
    invitationsLoading = true;
    try {
      const raw = await api.myInvitations();
      pendingInvitations = (raw?.invitations ?? []).filter(inv => inv.status === 'Pending');
    } catch {
      pendingInvitations = [];
    } finally {
      invitationsLoading = false;
    }
  }

  async function acceptInvitation(inv) {
    if (invitationBusyId) return;
    invitationBusyId = inv.id;
    try {
      await api.acceptWorkspaceInvitation(inv.workspace_id);
      showToast($t('user_profile.invitation_accepted'), { type: 'success' });
      await loadInvitations();
      // A new accepted membership may add a workspace — refresh the list.
      try { const raw = await api.workspaces(); workspaces = Array.isArray(raw?.workspaces) ? raw.workspaces : Array.isArray(raw) ? raw : []; } catch { /* keep existing */ }
    } catch (e) {
      showToast('Failed to accept invitation: ' + (e?.message ?? e), { type: 'error' });
    } finally {
      invitationBusyId = null;
    }
  }

  async function declineInvitation(inv) {
    if (invitationBusyId) return;
    invitationBusyId = inv.id;
    try {
      await api.declineWorkspaceInvitation(inv.workspace_id);
      showToast($t('user_profile.invitation_declined'), { type: 'info' });
      await loadInvitations();
    } catch (e) {
      showToast('Failed to decline invitation: ' + (e?.message ?? e), { type: 'error' });
    } finally {
      invitationBusyId = null;
    }
  }

  // ── Active sessions (user-management.md §UI Pages > Session
  // Management; endpoints from task-111) — devices with an active
  // session for the calling user, with per-session revoke and
  // "sign out everywhere". ──────────────────────────────────────────────
  let sessions = $state([]);
  let sessionsLoading = $state(false);
  let sessionBusyId = $state(null);
  let revokingAllSessions = $state(false);

  async function loadSessions() {
    sessionsLoading = true;
    try {
      const raw = await api.listMySessions();
      sessions = Array.isArray(raw?.sessions) ? raw.sessions : [];
    } catch {
      sessions = [];
    } finally {
      sessionsLoading = false;
    }
  }

  async function revokeSession(id) {
    if (sessionBusyId) return;
    sessionBusyId = id;
    try {
      await api.revokeMySession(id);
      sessions = sessions.filter(s => s.id !== id);
      showToast($t('user_profile.session_revoked'), { type: 'success' });
    } catch (e) {
      showToast($t('user_profile.session_revoke_failed') + (e?.message ? ': ' + e.message : ''), { type: 'error' });
    } finally {
      sessionBusyId = null;
    }
  }

  async function revokeAllSessions() {
    if (revokingAllSessions) return;
    revokingAllSessions = true;
    try {
      await api.revokeAllMySessions();
      sessions = [];
      showToast($t('user_profile.sessions_signed_out'), { type: 'success' });
    } catch (e) {
      showToast($t('user_profile.session_revoke_failed') + (e?.message ? ': ' + e.message : ''), { type: 'error' });
    } finally {
      revokingAllSessions = false;
    }
  }

  // task-111 timestamps are epoch seconds; WS-minted sessions carry no
  // User-Agent (no request headers on that path).
  function sessionTime(sec) {
    if (!Number.isFinite(sec) || sec <= 0) return '—';
    return new Date(sec * 1000).toLocaleString();
  }

  function sessionDevice(s) {
    return s?.user_agent?.trim() || $t('user_profile.session_ws_device');
  }

  // ── API Tokens ─────────────────────────────────────────────────────────
  let apiTokens = $state([]);
  let tokensLoading = $state(false);
  let newTokenName = $state('');
  let newTokenScopes = $state('read');
  let creatingToken = $state(false);
  let createdTokenValue = $state(null); // shown once after creation

  async function loadTokens() {
    tokensLoading = true;
    try {
      const data = await api.listApiTokens();
      apiTokens = Array.isArray(data) ? data : [];
    } catch {
      apiTokens = [];
    } finally {
      tokensLoading = false;
    }
  }

  async function createToken() {
    if (!newTokenName.trim() || creatingToken) return;
    creatingToken = true;
    try {
      const result = await api.createApiToken({ name: newTokenName.trim(), scopes: newTokenScopes.split(',').map(s => s.trim()).filter(Boolean) });
      createdTokenValue = result?.token ?? result?.value ?? null;
      newTokenName = '';
      showToast('API token created', { type: 'success' });
      await loadTokens();
    } catch (e) {
      showToast('Failed to create token: ' + (e?.message ?? e), { type: 'error' });
    } finally {
      creatingToken = false;
    }
  }

  async function revokeToken(id) {
    try {
      await api.deleteApiToken(id);
      apiTokens = apiTokens.filter(t => t.id !== id);
      showToast('Token revoked', { type: 'success' });
    } catch (e) {
      showToast('Failed to revoke token: ' + (e?.message ?? e), { type: 'error' });
    }
  }

  const tabs = $derived([
    { id: 'info',        label: $t('user_profile.tabs.info') },
    { id: 'tokens',      label: 'API Tokens' },
    { id: 'sessions',    label: $t('user_profile.tabs.sessions') },
    { id: 'memberships', label: $t('user_profile.tabs.memberships') },
    { id: 'invitations', label: $t('user_profile.tabs.invitations') },
    { id: 'ledger',      label: $t('user_profile.tabs.ledger') },
    { id: 'notif-prefs', label: $t('user_profile.tabs.notif_prefs') },
    { id: 'notifications', label: $t('user_profile.tabs.notifications') },
  ]);

  // ── Judgment Ledger: server-side filtering + pagination ──────────────────
  // GET /users/me/judgments implements ?type=/?workspace_id=/?limit=/
  // ?offset=; the Profile ledger tab must expose them or judgments beyond
  // the first page (default limit 50) are unreachable from the UI.
  const LEDGER_PAGE_SIZE = 50;
  const JUDGMENT_TYPE_FILTERS = [
    { id: '', label: 'All types' },
    { id: 'approval', label: 'Spec approvals' },
    { id: 'rejection', label: 'Rejections & withdrawals' },
    { id: 'gate', label: 'Gate overrides' },
    { id: 'trust', label: 'Trust changes' },
    { id: 'meta-spec', label: 'Meta-spec publishes' },
  ];
  let ledgerType = $state('');
  let ledgerWorkspace = $state('');
  let ledgerHasMore = $state(false);
  let ledgerLoading = $state(false);

  async function loadJudgments({ reset = false } = {}) {
    ledgerLoading = true;
    try {
      const params = { limit: LEDGER_PAGE_SIZE, offset: reset ? 0 : judgments.length };
      if (ledgerType) params.type = ledgerType;
      if (ledgerWorkspace) params.workspace_id = ledgerWorkspace;
      const raw = await api.myJudgments(params);
      const page = Array.isArray(raw?.judgments) ? raw.judgments : Array.isArray(raw?.items) ? raw.items : Array.isArray(raw) ? raw : [];
      judgments = reset ? page : [...judgments, ...page];
      // A full page means more entries may exist past this offset.
      ledgerHasMore = page.length === LEDGER_PAGE_SIZE;
    } catch {
      if (reset) judgments = [];
      ledgerHasMore = false;
    } finally {
      ledgerLoading = false;
    }
  }

  $effect(() => { loadAll(); });

  let invitationsLoaded = $state(false);

  // Load pending invitations when the tab is first selected. The
  // untrack'ed guard keeps loading-state writes from re-triggering the
  // effect (an empty result must not cause a reload loop).
  $effect(() => {
    if (activeTab === 'invitations') {
      if (untrack(() => !invitationsLoaded)) {
        invitationsLoaded = true;
        loadInvitations();
      }
    }
  });

  // Load API tokens when tab is selected
  $effect(() => {
    if (activeTab === 'tokens' && apiTokens.length === 0 && !tokensLoading) {
      loadTokens();
    }
  });

  let sessionsLoaded = $state(false);

  // Load active sessions when the tab is first selected (untrack'ed guard:
  // loading-state writes must not re-trigger the effect, same pattern as
  // the invitations tab).
  $effect(() => {
    if (activeTab === 'sessions') {
      if (untrack(() => !sessionsLoaded)) {
        sessionsLoaded = true;
        loadSessions();
      }
    }
  });

  async function loadAll() {
    loading = true;
    try {
      const [meR, ntR, wsR] = await Promise.allSettled([
        api.me(),
        api.myNotifications(),
        api.workspaces(),
      ]);
      if (meR.status === 'fulfilled') {
        me = meR.value;
        editForm = {
          display_name: me.display_name ?? '',
          timezone: me.timezone ?? '',
          locale: me.locale ?? '',
        };
        loadPrefsForm();
      }
      if (ntR.status === 'fulfilled') {
        const raw = ntR.value;
        notifications = Array.isArray(raw?.notifications) ? raw.notifications : Array.isArray(raw) ? raw : [];
      }
      // Badge = the server-side count of active notifications (resolved_at and
      // dismissed_at both NULL), filtered by the same disabled-type exclusion
      // the inbox list applies — NOT a client-side count of the current page.
      try {
        unread = await api.notificationCount();
      } catch {
        unread = 0;
      }
      // Judgment ledger: server-side filter + pagination (HSI §12 endpoint
      // query params) rather than a single unparameterized first page.
      loadJudgments({ reset: true });
      if (wsR.status === 'fulfilled') {
        const raw = wsR.value;
        workspaces = Array.isArray(raw?.workspaces) ? raw.workspaces : Array.isArray(raw) ? raw : [];
      }
      loadPrefs();
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
      showToast($t('user_profile.profile_updated'), { type: 'success' });
    } catch (e) {
      showToast('Failed to update profile: ' + e.message, { type: 'error' });
    } finally {
      saving = false;
    }
  }

  // A notification is "read" once resolved or dismissed (HSI §2: badge count
  // is resolved_at IS NULL AND dismissed_at IS NULL).
  function isNotifRead(n) {
    return Boolean(n?.resolved_at ?? n?.dismissed_at);
  }

  async function markRead(id) {
    try {
      await api.markNotificationRead(id);
      notifications = notifications.map(n => n.id === id ? { ...n, dismissed_at: Math.floor(Date.now() / 1000) } : n);
      unread = Math.max(0, unread - 1);
    } catch {
      showToast('Failed to mark notification as read', { type: 'error' });
    }
  }
  function judgmentEventColor(ev) {
    if (!ev) return 'default';
    const e = ev.toLowerCase();
    if (e.includes('approv')) return 'success';
    if (e.includes('reject') || e.includes('revok')) return 'danger';
    if (e.includes('gate')) return 'warning';
    if (e.includes('trust')) return 'warning';
    if (e.includes('meta')) return 'info';
    return 'neutral';
  }

  // Judgment entries are JudgmentEntryResponse:
  //   { judgment_type, entity_ref, workspace_id, timestamp, detail }
  function judgmentLabel(j) {
    return j.judgment_type ?? 'event';
  }

  function judgmentTarget(j) {
    return j.entity_ref ?? '—';
  }

  function judgmentWorkspace(j) {
    return j.workspace_id ?? null;
  }

  function notifColor(type) {
    const t = (type ?? '').toLowerCase();
    if (t.includes('failure') || t.includes('conflict') || t.includes('drift') || t.includes('violation') || t.includes('escalation')) return 'danger';
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

  // Auth provider info is read-only (HSI §12): rendered as the recorded
  // wall-clock timestamp, not a relative age — "Last Login" is a fact about
  // the account, not feed recency.
  function fmtLogin(ts) {
    if (!ts) return '—';
    const d = new Date(typeof ts === 'number' ? ts * 1000 : ts);
    return d.toLocaleString();
  }

  function switchWorkspace(ws) {
    goToWorkspaceHome?.(ws);
  }
</script>

<div class="user-profile">
  <div class="profile-header">
    <button class="back-btn" onclick={() => goToWorkspaceHome?.()} aria-label={$t('topbar.back_to_workspace')} title={$t('topbar.back_to_workspace')}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <path d="M19 12H5"/><path d="M12 19l-7-7 7-7"/>
      </svg>
    </button>
    <div class="avatar" aria-hidden="true">
      {me?.display_name?.[0]?.toUpperCase() ?? me?.username?.[0]?.toUpperCase() ?? '?'}
    </div>
    <div class="profile-meta">
      <h1 class="page-title">{me?.display_name ?? me?.username ?? $t('user_profile.title')}</h1>
      {#if me?.username}
        <button class="username username-link" onclick={() => goToUserProfile?.(me.username)} title={$t('user_profile.view_public_profile')} data-testid="public-profile-link">@{me.username}</button>
      {/if}
      {#if me?.global_role}
        <Badge variant="info" value={me.global_role} />
      {/if}
    </div>
    {#if !editing}
      <button class="btn-edit" onclick={() => (editing = true)}>{$t('user_profile.edit')}</button>
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
    <form class="edit-form" onsubmit={(e) => { e.preventDefault(); saveEdit(); }}>
      <label class="field-label">{$t('user_profile.fields.display_name')}
        <input class="field-input" bind:value={editForm.display_name} placeholder={$t('user_profile.placeholders.display_name')} autocomplete="name" />
      </label>
      <label class="field-label">{$t('user_profile.fields.timezone')}
        <input class="field-input" bind:value={editForm.timezone} placeholder={$t('user_profile.placeholders.timezone')} autocomplete="off" />
      </label>
      <label class="field-label">{$t('user_profile.fields.locale')}
        <input class="field-input" bind:value={editForm.locale} placeholder={$t('user_profile.placeholders.locale')} autocomplete="off" />
      </label>
      <fieldset class="prefs-fieldset">
        <legend class="field-label">{$t('user_profile.prefs_section_title')}</legend>
        <div class="prefs-grid">
          <label class="field-label">{$t('user_profile.prefs.theme')}
            <select class="field-input" bind:value={prefsForm.theme} data-testid="prefs-theme">
              {#each THEMES as theme}<option value={theme}>{theme}</option>{/each}
            </select>
          </label>
          <label class="field-label">{$t('user_profile.prefs.ui_density')}
            <select class="field-input" bind:value={prefsForm.ui_density} data-testid="prefs-density">
              {#each DENSITIES as d}<option value={d}>{d}</option>{/each}
            </select>
          </label>
          <label class="field-label">{$t('user_profile.prefs.diff_view')}
            <select class="field-input" bind:value={prefsForm.diff_view} data-testid="prefs-diff-view">
              {#each DIFF_VIEWS as dv}<option value={dv}>{dv === 'SideBySide' ? $t('user_profile.prefs.diff_side_by_side') : $t('user_profile.prefs.diff_unified')}</option>{/each}
            </select>
          </label>
          <label class="field-label">{$t('user_profile.prefs.feed_scope')}
            <select class="field-input" bind:value={prefsForm.activity_feed_scope} data-testid="prefs-feed-scope">
              {#each FEED_SCOPES as fs}<option value={fs}>{fs}</option>{/each}
            </select>
          </label>
          <label class="field-label">{$t('user_profile.prefs.code_font_size')}
            <input class="field-input" type="number" min="10" max="24" step="1" bind:value={prefsForm.code_font_size} data-testid="prefs-font-size" />
          </label>
        </div>
        <button type="button" class="btn-secondary prefs-save-btn" onclick={savePrefsForm} disabled={prefsFormSaving} data-testid="prefs-save-btn">
          {prefsFormSaving ? $t('agent_card.saving') : $t('user_profile.save_prefs_only')}
        </button>
      </fieldset>
      <div class="edit-actions">
        <button type="button" class="btn-secondary" onclick={() => { editForm = { display_name: me?.display_name ?? '', timezone: me?.timezone ?? '', locale: me?.locale ?? '' }; editing = false; }}>{$t('common.cancel')}</button>
        <button type="button" class="btn-primary" onclick={saveEdit} disabled={saving || !editForm.display_name.trim()} aria-busy={saving}>
          {saving ? $t('agent_card.saving') : $t('common.save')}
        </button>
      </div>
    </form>
  {/if}

  <Tabs {tabs} bind:active={activeTab} />

  <div class="tab-body" role="tabpanel" id="tabpanel-{activeTab}" aria-labelledby="tab-{activeTab}" aria-busy={loading}>
    {#if loading}
      <Skeleton lines={5} />
    {:else if activeTab === 'info'}
      {#if me}
        <div class="info-grid">
          {#each [[$t('user_profile.fields.username'), me.username],[$t('user_profile.fields.email'), me.email],[$t('user_profile.fields.display_name'), me.display_name],[$t('user_profile.fields.timezone'), me.timezone],[$t('user_profile.fields.locale'), me.locale],[$t('user_profile.fields.role'), me.global_role],[$t('user_profile.fields.auth_provider'), me.oidc_issuer],[$t('user_profile.fields.last_login'), fmtLogin(me.last_login_at)]] as [label, val]}
            {#if val}
              <div class="info-row">
                <span class="info-label">{label}</span>
                <span class="info-val">{val}</span>
              </div>
            {/if}
          {/each}
        </div>
      {:else}
        <EmptyState title={$t('user_profile.no_profile')} description={$t('user_profile.no_profile_desc')} />
      {/if}

    {:else if activeTab === 'tokens'}
      <!-- API Tokens management -->
      <div class="tokens-section">
        <p class="tokens-desc">API tokens allow programmatic access to the Gyre API. Tokens are scoped and can be revoked at any time.</p>

        {#if createdTokenValue}
          <div class="token-created-banner">
            <p class="token-created-title">Token created — copy it now, it won't be shown again</p>
            <div class="token-created-value">
              <code class="token-value mono">{createdTokenValue}</code>
              <button class="token-copy-btn" onclick={async () => { try { await navigator.clipboard.writeText(createdTokenValue); showToast('Token copied', { type: 'success' }); } catch {} }}>Copy</button>
            </div>
            <button class="token-dismiss-btn" onclick={() => { createdTokenValue = null; }}>Dismiss</button>
          </div>
        {/if}

        <form class="token-create-form" onsubmit={(e) => { e.preventDefault(); createToken(); }}>
          <input class="token-input" type="text" placeholder="Token name (e.g. ci-pipeline)" bind:value={newTokenName} required />
          <select class="token-scope-select" bind:value={newTokenScopes}>
            <option value="read">Read only</option>
            <option value="read,write">Read + Write</option>
            <option value="read,write,admin">Full access</option>
          </select>
          <button class="token-create-btn" type="submit" disabled={creatingToken || !newTokenName.trim()}>
            {creatingToken ? 'Creating...' : 'Create Token'}
          </button>
        </form>

        {#if tokensLoading}
          <Skeleton width="100%" height="2rem" />
        {:else if apiTokens.length === 0}
          <p class="tokens-empty">No API tokens. Create one above to get started.</p>
        {:else}
          <div class="tokens-list">
            {#each apiTokens as token}
              <div class="token-item">
                <div class="token-item-info">
                  <span class="token-item-name">{token.name ?? 'Unnamed'}</span>
                  {#if token.scopes?.length > 0}
                    <span class="token-item-scopes">{token.scopes.join(', ')}</span>
                  {/if}
                  {#if token.created_at}
                    <span class="token-item-created">Created {shortId(token.id)}</span>
                  {/if}
                </div>
                <button class="token-revoke-btn" onclick={() => revokeToken(token.id)} title="Revoke this token">Revoke</button>
              </div>
            {/each}
          </div>
        {/if}
      </div>

    {:else if activeTab === 'sessions'}
      <!-- Active sessions (user-management.md §UI Pages > Session Management,
           endpoints from task-111) -->
      <div class="sessions-section">
        <p class="sessions-desc">{$t('user_profile.sessions_desc')}</p>

        <div class="sessions-actions">
          <button
            class="sessions-revoke-all-btn"
            data-testid="sessions-revoke-all"
            onclick={revokeAllSessions}
            disabled={revokingAllSessions || sessionsLoading || sessions.length === 0}
            title={$t('user_profile.sessions_revoke_all_title')}
          >
            {revokingAllSessions ? $t('user_profile.sessions_signing_out') : $t('user_profile.sessions_revoke_all')}
          </button>
        </div>

        {#if sessionsLoading}
          <Skeleton width="100%" height="2rem" />
        {:else if sessions.length === 0}
          <p class="sessions-empty">{$t('user_profile.no_sessions')}</p>
        {:else}
          <div class="sessions-list">
            {#each sessions as s (s.id)}
              <div class="session-item" data-testid={`session-${s.id}`}>
                <div class="session-item-info">
                  <span class="session-item-device">{sessionDevice(s)}</span>
                  <span class="session-item-meta">{s.ip_address || '—'} · {$t('user_profile.session_last_active')} {sessionTime(s.last_active_at)}</span>
                  <span class="session-item-meta">{$t('user_profile.session_started')} {sessionTime(s.created_at)} · {$t('user_profile.session_expires')} {sessionTime(s.expires_at)}</span>
                </div>
                <button
                  class="session-revoke-btn"
                  data-testid={`session-revoke-${s.id}`}
                  onclick={() => revokeSession(s.id)}
                  disabled={sessionBusyId === s.id}
                  title={$t('user_profile.session_revoke_title')}
                >
                  {sessionBusyId === s.id ? $t('user_profile.session_revoking') : $t('user_profile.session_revoke')}
                </button>
              </div>
            {/each}
          </div>
        {/if}
      </div>

    {:else if activeTab === 'memberships'}
      <!-- Workspace memberships with quick-switch -->
      {#if workspaces.length === 0}
        <EmptyState title={$t('user_profile.no_workspaces')} description={$t('user_profile.no_workspaces_desc')} />
      {:else}
        <div class="memberships-list">
          {#each workspaces as ws}
            <div class="membership-item">
              <div class="membership-info">
                <span class="membership-name" title={ws.name ?? ws.slug ?? ws.id}>{ws.name ?? ws.slug ?? shortId(ws.id)}</span>
                {#if ws.role}
                  <Badge value={ws.role} variant="muted" />
                {/if}
              </div>
              {#if ws.trust_level}
                <span class="membership-trust muted">{$t('user_profile.trust_label', { values: { level: ws.trust_level } })}</span>
              {/if}
              <button
                class="btn-switch"
                onclick={() => switchWorkspace(ws)}
                aria-label={$t('user_profile.switch_workspace', { values: { name: ws.name ?? ws.id } })}
              >
                {$t('user_profile.switch')}
              </button>
            </div>
          {/each}
        </div>
      {/if}

    {:else if activeTab === 'invitations'}
      <!-- Pending workspace invitations: accept / decline (task-110
           invitee endpoints; the invitee-only decline is §Workspace
           Invitation Flow step 4). -->
      {#if invitationsLoading}
        <Skeleton lines={3} />
      {:else if pendingInvitations.length === 0}
        <EmptyState title={$t('user_profile.no_invitations')} description={$t('user_profile.no_invitations_desc')} />
      {:else}
        <div class="invitations-list" data-testid="pending-invitations">
          {#each pendingInvitations as inv (inv.id)}
            <div class="invitation-item" data-testid={`invitation-${inv.id}`}>
              <div class="invitation-info">
                <span class="invitation-ws mono" title={inv.workspace_id}>{inv.workspace_id}</span>
                <Badge variant="info" value={inv.role} />
                <span class="muted">{rel(inv.created_at)}</span>
              </div>
              <div class="invitation-actions">
                <button class="btn-primary" onclick={() => acceptInvitation(inv)} disabled={invitationBusyId === inv.id} data-testid={`invitation-accept-${inv.id}`}>
                  {invitationBusyId === inv.id ? '…' : $t('user_profile.accept_invitation')}
                </button>
                <button class="btn-secondary" onclick={() => declineInvitation(inv)} disabled={invitationBusyId === inv.id} data-testid={`invitation-decline-${inv.id}`}>
                  {$t('user_profile.decline_invitation')}
                </button>
              </div>
            </div>
          {/each}
        </div>
      {/if}

    {:else if activeTab === 'ledger'}
      <!-- Judgment Ledger: chronological log of human judgment decisions -->
      <!-- Sourced from GET /api/v1/users/me/judgments (?type=/?workspace_id=/?limit=/?offset=) -->
      <div class="ledger-controls">
        <label class="ledger-filter">
          <span class="ledger-filter-label muted">Type</span>
          <select
            class="ledger-select"
            bind:value={ledgerType}
            onchange={() => loadJudgments({ reset: true })}
            aria-label="Filter judgments by type"
          >
            {#each JUDGMENT_TYPE_FILTERS as jtf}
              <option value={jtf.id}>{jtf.label}</option>
            {/each}
          </select>
        </label>
        <label class="ledger-filter">
          <span class="ledger-filter-label muted">Workspace</span>
          <select
            class="ledger-select"
            bind:value={ledgerWorkspace}
            onchange={() => loadJudgments({ reset: true })}
            aria-label="Filter judgments by workspace"
          >
            <option value="">All workspaces</option>
            {#each workspaces as ws}
              <option value={ws.id}>{ws.name ?? ws.slug ?? ws.id}</option>
            {/each}
          </select>
        </label>
      </div>
      {#if judgments.length === 0 && !ledgerLoading}
        <EmptyState title={$t('user_profile.no_activity')} description={$t('user_profile.no_activity_desc')} />
      {:else}
        <div class="ledger-list">
          {#each judgments as j}
            <div class="ledger-item">
              <div class="ledger-row">
                <Badge value={judgmentLabel(j)} variant={judgmentEventColor(judgmentLabel(j))} />
                <span class="ledger-target mono" title={judgmentTarget(j)}>{judgmentTarget(j)}</span>
                <span class="ledger-time muted">{rel(j.timestamp ?? j.created_at ?? j.approved_at)}</span>
              </div>
              <div class="ledger-meta">
                {#if judgmentWorkspace(j)}
                  <span class="ledger-ws muted">{judgmentWorkspace(j)}</span>
                {/if}
                {#if j.detail}
                  <span class="ledger-sha muted">{j.detail}</span>
                {/if}
              </div>
            </div>
          {/each}
        </div>
        {#if ledgerHasMore}
          <button class="btn-secondary ledger-more" onclick={() => loadJudgments()} disabled={ledgerLoading}>
            {ledgerLoading ? 'Loading…' : 'Load more'}
          </button>
        {/if}
      {/if}

    {:else if activeTab === 'notif-prefs'}
      <!-- Notification Preferences: per-type toggles (HSI §12) + delivery
           channels (user-management.md §Delivery Channels). -->
      <div class="prefs-section">
        <p class="prefs-desc">{$t('user_profile.prefs_desc')}</p>
        <div class="prefs-list">
          {#each NOTIF_TYPES as nt}
            <label class="pref-row">
              <input
                type="checkbox"
                class="pref-checkbox"
                bind:checked={notifPrefs[nt.id]}
                aria-label={$t('user_profile.enable_notification', { values: { label: $t(nt.labelKey) } })}
              />
              <span class="pref-label">{$t(nt.labelKey)}</span>
            </label>
          {/each}
        </div>

        <h3 class="channels-title">{$t('user_profile.channels_title')}</h3>
        <p class="prefs-desc">{$t('user_profile.channels_desc')}</p>
        <div class="channels-section" data-testid="notification-channels">
          <div class="channel-row">
            <span class="channel-name">{$t('user_profile.channel_in_app')}</span>
            <input type="checkbox" checked disabled aria-label={$t('user_profile.channel_in_app')} title={$t('user_profile.channel_in_app_locked')} />
          </div>
          <div class="channel-block">
            <div class="channel-row">
              <span class="channel-name">{$t('user_profile.channel_email')}</span>
              <input type="checkbox" bind:checked={channels.email.enabled} data-testid="channel-email-enabled" aria-label={$t('user_profile.channel_email')} />
            </div>
            {#if channels.email.enabled}
              <div class="channel-config">
                <label class="field-label">{$t('user_profile.channel_digest')}
                  <select class="field-input" bind:value={channels.email.digest} data-testid="channel-email-digest">
                    {#each DIGESTS as d}<option value={d}>{d}</option>{/each}
                  </select>
                </label>
                <label class="field-label">{$t('user_profile.channel_min_priority')}
                  <select class="field-input" bind:value={channels.email.min_priority} data-testid="channel-email-min-priority">
                    {#each PRIORITIES as p}<option value={p}>{p}</option>{/each}
                  </select>
                </label>
              </div>
            {/if}
          </div>
          <div class="channel-block">
            <div class="channel-row">
              <span class="channel-name">{$t('user_profile.channel_webhook')}</span>
              <span class="muted">{channels.webhook ? $t('user_profile.channel_configured') : $t('user_profile.channel_not_configured')}</span>
            </div>
            <div class="channel-config">
              <label class="field-label">{$t('user_profile.channel_url')}
                <input class="field-input" type="url" bind:value={webhookDraft.url} placeholder="https://…" data-testid="channel-webhook-url" />
              </label>
              <label class="field-label">{$t('user_profile.channel_secret')}
                <input class="field-input" type="password" bind:value={webhookDraft.secret} placeholder="…" data-testid="channel-webhook-secret" />
              </label>
              <label class="field-label">{$t('user_profile.channel_min_priority')}
                <select class="field-input" bind:value={webhookDraft.min_priority}>
                  {#each PRIORITIES as p}<option value={p}>{p}</option>{/each}
                </select>
              </label>
            </div>
          </div>
          <div class="channel-block">
            <div class="channel-row">
              <span class="channel-name">{$t('user_profile.channel_slack')}</span>
              <span class="muted">{channels.slack ? $t('user_profile.channel_configured') : $t('user_profile.channel_not_configured')}</span>
            </div>
            <div class="channel-config">
              <label class="field-label">{$t('user_profile.channel_url')}
                <input class="field-input" type="url" bind:value={slackDraft.webhook_url} placeholder="https://hooks.slack.com/…" data-testid="channel-slack-url" />
              </label>
              <label class="field-label">{$t('user_profile.channel_slack_channel')}
                <input class="field-input" bind:value={slackDraft.channel} placeholder="#team" data-testid="channel-slack-channel" />
              </label>
              <label class="field-label">{$t('user_profile.channel_min_priority')}
                <select class="field-input" bind:value={slackDraft.min_priority}>
                  {#each PRIORITIES as p}<option value={p}>{p}</option>{/each}
                </select>
              </label>
            </div>
          </div>
        </div>
        <div class="prefs-actions">
          <button class="btn-primary" onclick={savePrefs} disabled={prefsSaving} aria-busy={prefsSaving} data-testid="save-notification-prefs">
            {prefsSaving ? $t('agent_card.saving') : $t('user_profile.save_prefs')}
          </button>
        </div>
      </div>

    {:else if activeTab === 'notifications'}
      {#if notifications.length === 0}
        <EmptyState title={$t('user_profile.no_notifications')} description={$t('user_profile.no_notifications_desc')} />
      {:else}
        <div class="notif-list">
          {#each notifications as notif}
            <div class="notif-item" class:unread={!isNotifRead(notif)}>
              <div class="notif-top">
                <Badge variant={notifColor(notif.notification_type)} value={notif.notification_type ?? 'info'} />
                <span class="notif-time muted">{rel(notif.created_at)}</span>
                {#if !isNotifRead(notif)}
                  <button class="mark-read-btn" onclick={() => markRead(notif.id)} aria-label={$t('user_profile.mark_as_read')}>
                    <span aria-hidden="true">✓</span>
                  </button>
                {/if}
              </div>
              <p class="notif-msg">{notif.message ?? notif.title ?? ''}</p>
              <button class="notif-view-btn" onclick={() => { goToWorkspaceHome?.(); }} aria-label={$t('user_profile.view_in_inbox_label')}>
                {$t('user_profile.view_in_inbox')}
              </button>
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

  .back-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    flex-shrink: 0;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }
  .back-btn:hover { color: var(--color-text); border-color: var(--color-text-muted); }
  .back-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .avatar {
    width: var(--space-12);
    height: var(--space-12);
    border-radius: 50%;
    background: color-mix(in srgb, var(--color-focus) 15%, transparent);
    color: var(--color-focus);
    font-weight: 700;
    font-size: var(--text-xl);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .profile-meta { flex: 1; }
  .profile-meta .page-title { margin: 0 0 var(--space-1); font-size: var(--text-xl); font-weight: 600; color: var(--color-text); }
  .username { margin: 0 0 var(--space-1); font-size: var(--text-sm); color: var(--color-text-muted); font-family: var(--font-mono); }
  .username-link { background: none; border: none; padding: 0; cursor: pointer; font: inherit; color: inherit; }
  .username-link:hover { color: var(--color-primary); text-decoration: underline; }
  .prefs-fieldset { border: 1px solid var(--color-border); border-radius: var(--radius); padding: var(--space-3); display: flex; flex-direction: column; gap: var(--space-2); }
  .prefs-fieldset legend { padding: 0 var(--space-1); }
  .prefs-grid { display: flex; flex-wrap: wrap; gap: var(--space-2); }
  .prefs-save-btn { align-self: flex-start; }
  .invitations-list { display: flex; flex-direction: column; gap: var(--space-2); }
  .invitation-item { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); border: 1px solid var(--color-border); border-radius: var(--radius); padding: var(--space-2) var(--space-3); }
  .invitation-info { display: flex; align-items: center; gap: var(--space-2); min-width: 0; }
  .invitation-ws { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .invitation-actions { display: flex; gap: var(--space-2); flex-shrink: 0; }
  .channels-title { margin: var(--space-3) 0 0; font-size: var(--text-sm); }
  .channels-section { display: flex; flex-direction: column; gap: var(--space-2); border: 1px solid var(--color-border); border-radius: var(--radius); padding: var(--space-3); }
  .channel-block { display: flex; flex-direction: column; gap: var(--space-2); border-top: 1px dashed var(--color-border); padding-top: var(--space-2); }
  .channel-row { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); }
  .channel-name { font-weight: 500; font-size: var(--text-sm); }
  .channel-config { display: flex; flex-wrap: wrap; gap: var(--space-2); }

  .btn-edit {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }
  .btn-edit:hover { border-color: var(--color-text-muted); }

  .notif-bell { position: relative; display: flex; align-items: center; color: var(--color-text-muted); }
  .notif-count {
    position: absolute;
    top: -4px;
    right: -6px;
    background: var(--color-danger);
    color: var(--color-text-inverse);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    padding: 0 var(--space-1);
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
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-color: var(--color-focus);
  }

  .edit-actions { display: flex; gap: var(--space-2); align-items: center; }

  .btn-primary {
    padding: var(--space-3) var(--space-4);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }
  .btn-primary:hover:not(:disabled) { background: var(--color-primary-hover); }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

  .btn-secondary {
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }
  .btn-secondary:hover { border-color: var(--color-text-muted); color: var(--color-text); }

  .tab-body { flex: 1; overflow-y: auto; padding: var(--space-6); }

  /* Profile info */
  .info-grid { display: flex; flex-direction: column; gap: var(--space-3); max-width: 480px; }
  .info-row { display: flex; gap: var(--space-4); font-size: var(--text-sm); }
  .info-label { color: var(--color-text-muted); width: 120px; flex-shrink: 0; }
  .info-val { color: var(--color-text); }

  /* Workspace memberships */
  /* ── API Tokens ───────────────────────────────────────────────────────── */
  .tokens-section { display: flex; flex-direction: column; gap: var(--space-3); }
  .sessions-section { display: flex; flex-direction: column; gap: var(--space-3); }
  .sessions-desc { margin: 0; font-size: var(--text-sm); color: var(--color-text-muted); }
  .sessions-actions { display: flex; justify-content: flex-end; }
  .sessions-revoke-all-btn { background: transparent; border: 1px solid color-mix(in srgb, var(--color-danger) 30%, var(--color-border)); border-radius: var(--radius-sm); padding: var(--space-1) var(--space-3); font-size: var(--text-sm); cursor: pointer; color: var(--color-danger); font-family: var(--font-body); font-weight: 600; }
  .sessions-revoke-all-btn:hover { background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .sessions-revoke-all-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .sessions-empty { font-size: var(--text-sm); color: var(--color-text-muted); font-style: italic; text-align: center; padding: var(--space-4) 0; }
  .sessions-list { display: flex; flex-direction: column; gap: var(--space-1); }
  .session-item { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-surface); }
  .session-item-info { display: flex; flex-direction: column; gap: 2px; flex: 1; min-width: 0; }
  .session-item-device { font-weight: 600; font-size: var(--text-sm); color: var(--color-text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .session-item-meta { font-size: var(--text-xs); color: var(--color-text-muted); }
  .session-revoke-btn { background: transparent; border: 1px solid color-mix(in srgb, var(--color-danger) 30%, var(--color-border)); border-radius: var(--radius-sm); padding: var(--space-1) var(--space-2); font-size: var(--text-xs); cursor: pointer; color: var(--color-danger); font-family: var(--font-body); font-weight: 600; white-space: nowrap; }
  .session-revoke-btn:hover { background: color-mix(in srgb, var(--color-danger) 8%, transparent); }
  .session-revoke-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .tokens-desc { margin: 0; font-size: var(--text-sm); color: var(--color-text-muted); }
  .token-created-banner { background: color-mix(in srgb, var(--color-success) 8%, var(--color-surface)); border: 1px solid color-mix(in srgb, var(--color-success) 30%, var(--color-border)); border-radius: var(--radius); padding: var(--space-3); display: flex; flex-direction: column; gap: var(--space-2); }
  .token-created-title { margin: 0; font-size: var(--text-sm); font-weight: 600; color: var(--color-success); }
  .token-created-value { display: flex; gap: var(--space-2); align-items: center; }
  .token-value { font-size: var(--text-sm); background: var(--color-surface-elevated); padding: var(--space-2); border-radius: var(--radius-sm); overflow-x: auto; flex: 1; user-select: all; word-break: break-all; }
  .token-copy-btn, .token-dismiss-btn { background: transparent; border: 1px solid var(--color-border); border-radius: var(--radius-sm); padding: var(--space-1) var(--space-2); font-size: var(--text-xs); cursor: pointer; color: var(--color-link); font-family: var(--font-body); }
  .token-copy-btn:hover, .token-dismiss-btn:hover { border-color: var(--color-primary); }
  .token-dismiss-btn { align-self: flex-start; color: var(--color-text-muted); }
  .token-create-form { display: flex; gap: var(--space-2); align-items: center; flex-wrap: wrap; }
  .token-input { flex: 1; min-width: 180px; padding: var(--space-2); border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-surface); color: var(--color-text); font-family: var(--font-body); font-size: var(--text-sm); }
  .token-scope-select { padding: var(--space-2); border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-surface); color: var(--color-text); font-family: var(--font-body); font-size: var(--text-sm); }
  .token-create-btn { padding: var(--space-2) var(--space-3); background: var(--color-primary); color: white; border: none; border-radius: var(--radius-sm); font-size: var(--text-sm); font-weight: 600; cursor: pointer; font-family: var(--font-body); white-space: nowrap; }
  .token-create-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .tokens-empty { font-size: var(--text-sm); color: var(--color-text-muted); font-style: italic; text-align: center; padding: var(--space-4) 0; }
  .tokens-list { display: flex; flex-direction: column; gap: var(--space-1); }
  .token-item { display: flex; align-items: center; justify-content: space-between; gap: var(--space-2); padding: var(--space-2) var(--space-3); border: 1px solid var(--color-border); border-radius: var(--radius-sm); background: var(--color-surface); }
  .token-item-info { display: flex; align-items: center; gap: var(--space-2); flex: 1; min-width: 0; }
  .token-item-name { font-weight: 600; font-size: var(--text-sm); color: var(--color-text); }
  .token-item-scopes { font-size: var(--text-xs); color: var(--color-text-muted); font-family: var(--font-mono); }
  .token-item-created { font-size: var(--text-xs); color: var(--color-text-muted); }
  .token-revoke-btn { background: transparent; border: 1px solid color-mix(in srgb, var(--color-danger) 30%, var(--color-border)); border-radius: var(--radius-sm); padding: var(--space-1) var(--space-2); font-size: var(--text-xs); cursor: pointer; color: var(--color-danger); font-family: var(--font-body); font-weight: 600; }
  .token-revoke-btn:hover { background: color-mix(in srgb, var(--color-danger) 8%, transparent); }

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
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }
  .btn-switch:hover { border-color: var(--color-link); color: var(--color-link); }

  /* Judgment Ledger */
  .ledger-controls { display: flex; gap: var(--space-3); flex-wrap: wrap; margin-bottom: var(--space-3); }
  .ledger-filter { display: flex; flex-direction: column; gap: var(--space-1); font-size: var(--text-xs); }
  .ledger-filter-label { font-weight: 500; }
  .ledger-select {
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    color: var(--color-text);
    font-size: var(--text-sm);
    font-family: var(--font-body);
  }
  .ledger-select:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .ledger-more { align-self: flex-start; margin-top: var(--space-2); padding: var(--space-1) var(--space-3); font-size: var(--text-sm); }
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
  .pref-checkbox { accent-color: var(--color-focus); width: 16px; height: 16px; cursor: pointer; }
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
  .notif-item.unread { border-left: 3px solid var(--color-focus); }
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
    padding: var(--space-1);
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }
  .mark-read-btn:hover { color: var(--color-success); border-color: var(--color-success); }

  .notif-view-btn {
    background: transparent;
    border: none;
    color: var(--color-link);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    padding: 0;
    transition: color var(--transition-fast);
  }
  .notif-view-btn:hover { color: var(--color-link-hover); }
  .notif-view-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .mono { font-family: var(--font-mono); }
  .muted { color: var(--color-text-muted); }

  .btn-edit:focus-visible,
  .btn-secondary:focus-visible,
  .btn-primary:focus-visible,
  .btn-switch:focus-visible,
  .mark-read-btn:focus-visible,
  .pref-checkbox:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .mark-read-btn,
    .back-btn,
    .notif-view-btn { transition: none; }
  }
</style>
