<script>
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Tabs from '../lib/Tabs.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';
  import { shortId, entityName } from '../lib/entityNames.svelte.js';
  import { getContext } from 'svelte';

  const navigate = getContext('navigate');
  const goToWorkspaceHome = getContext('goToWorkspaceHome');
  const openDetailPanel = getContext('openDetailPanel');
  const goToEntityDetail = getContext('goToEntityDetail') ?? null;

  function nav(type, id, data) {
    if (goToEntityDetail) goToEntityDetail(type, id, data ?? {});
    else if (openDetailPanel) openDetailPanel({ type, id, data: data ?? {} });
  }

  // ── State ──────────────────────────────────────────────────────────────────
  let me = $state(null);
  let notifications = $state([]);
  let judgments = $state([]);
  let workspaces = $state([]);
  let myAgents = $state([]);
  let myTasks = $state([]);
  let myMrs = $state([]);
  let loading = $state(true);
  let editing = $state(false);
  let saving = $state(false);
  let editForm = $state({ display_name: '', timezone: '', locale: '' });
  let activeTab = $state('info');
  let unread = $state(0);

  // Notification preferences — per-type toggles.
  // Per HSI §12, stored in user_notification_preferences table (backend).
  // Using localStorage as fallback until backend endpoint is wired.
  const NOTIF_TYPE_IDS = [
    'SpecApproval', 'GateOverride', 'TrustChange', 'MetaSpecEdit',
    'MergeRequestReview', 'MergeRequestMerged', 'AgentFailure',
    'TrustSuggestion', 'SpecDrift', 'AgentNeedsClarification',
  ];
  // Build NOTIF_TYPES array with localized labels for compatibility with defaultPrefs()
  const NOTIF_TYPES = NOTIF_TYPE_IDS.map(id => ({ id, labelKey: `user_profile.notif_types.${id}` }));

  function defaultPrefs() {
    return Object.fromEntries(NOTIF_TYPES.map(t => [t.id, true]));
  }

  let notifPrefs = $state(defaultPrefs());
  let prefsSaving = $state(false);
  let prefsLoaded = $state(false);

  async function loadPrefs() {
    try {
      const serverPrefs = await api.getNotificationPreferences();
      if (serverPrefs && typeof serverPrefs === 'object' && !Array.isArray(serverPrefs)) {
        const defaults = defaultPrefs();
        for (const t of NOTIF_TYPES) {
          if (typeof serverPrefs[t.id] === 'boolean') defaults[t.id] = serverPrefs[t.id];
        }
        notifPrefs = defaults;
      }
    } catch {
      // Server may not support this yet — fall back to defaults
    }
    prefsLoaded = true;
  }

  async function savePrefs() {
    prefsSaving = true;
    try {
      await api.updateNotificationPreferences(notifPrefs);
      showToast($t('user_profile.prefs_saved'), { type: 'success' });
    } catch {
      showToast($t('user_profile.prefs_failed'), { type: 'error' });
    }
    prefsSaving = false;
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
    { id: 'my-agents',   label: `Agents${myAgents.length > 0 ? ` (${myAgents.length})` : ''}` },
    { id: 'my-tasks',    label: `Tasks${myTasks.length > 0 ? ` (${myTasks.length})` : ''}` },
    { id: 'my-mrs',      label: `MRs${myMrs.length > 0 ? ` (${myMrs.length})` : ''}` },
    { id: 'tokens',      label: 'API Tokens' },
    { id: 'memberships', label: $t('user_profile.tabs.memberships') },
    { id: 'ledger',      label: $t('user_profile.tabs.ledger') },
    { id: 'notif-prefs', label: $t('user_profile.tabs.notif_prefs') },
    { id: 'notifications', label: $t('user_profile.tabs.notifications') },
  ]);

  $effect(() => { loadAll(); });

  // Load API tokens when tab is selected
  $effect(() => {
    if (activeTab === 'tokens' && apiTokens.length === 0 && !tokensLoading) {
      loadTokens();
    }
  });

  async function loadAll() {
    loading = true;
    try {
      const [meR, ntR, jdR, wsR, agR, tkR, mrR] = await Promise.allSettled([
        api.me(),
        api.myNotifications(),
        api.myJudgments(),
        api.workspaces(),
        api.myAgents(),
        api.myTasks(),
        api.myMrs(),
      ]);
      if (meR.status === 'fulfilled') {
        me = meR.value;
        editForm = {
          display_name: me.display_name ?? '',
          timezone: me.timezone ?? '',
          locale: me.locale ?? '',
        };
      }
      if (agR.status === 'fulfilled') {
        myAgents = Array.isArray(agR.value) ? agR.value : [];
      }
      if (tkR.status === 'fulfilled') {
        myTasks = Array.isArray(tkR.value) ? tkR.value : [];
      }
      if (mrR.status === 'fulfilled') {
        myMrs = Array.isArray(mrR.value) ? mrR.value : [];
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
        <p class="username">@{me.username}</p>
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
      <div class="edit-actions">
        <button class="btn-secondary" onclick={() => { editForm = { display_name: me?.display_name ?? '', timezone: me?.timezone ?? '', locale: me?.locale ?? '' }; editing = false; }}>{$t('common.cancel')}</button>
        <button class="btn-primary" onclick={saveEdit} disabled={saving || !editForm.display_name.trim()} aria-busy={saving}>
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
          {#each [[$t('user_profile.fields.username'), me.username],[$t('user_profile.fields.email'), me.email],[$t('user_profile.fields.display_name'), me.display_name],[$t('user_profile.fields.timezone'), me.timezone],[$t('user_profile.fields.locale'), me.locale],[$t('user_profile.fields.role'), me.global_role],[$t('user_profile.fields.auth_provider'), me.oidc_issuer]] as [label, val]}
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

    {:else if activeTab === 'my-agents'}
      {#if myAgents.length === 0}
        <EmptyState title="No agents" description="You haven't spawned any agents yet." />
      {:else}
        <div class="entity-list">
          {#each myAgents as agent}
            {@const statusColor = agent.status === 'active' ? 'success' : agent.status === 'idle' || agent.status === 'completed' ? 'info' : agent.status === 'failed' || agent.status === 'dead' ? 'danger' : 'muted'}
            <button class="entity-list-item" onclick={() => nav('agent', agent.id, agent)}>
              <Badge value={agent.status ?? 'unknown'} variant={statusColor} />
              <div class="entity-list-main">
                <span class="entity-list-title">{agent.name ?? entityName('agent', agent.id)}</span>
                {#if agent.branch}
                  <span class="entity-list-sub mono">{agent.branch}</span>
                {/if}
              </div>
              <span class="entity-list-time muted">{rel(agent.created_at ?? agent.spawned_at)}</span>
            </button>
          {/each}
        </div>
      {/if}

    {:else if activeTab === 'my-tasks'}
      {#if myTasks.length === 0}
        <EmptyState title="No tasks" description="No tasks assigned to you." />
      {:else}
        <div class="entity-list">
          {#each myTasks as task}
            {@const statusColor = task.status === 'completed' || task.status === 'done' ? 'success' : task.status === 'in_progress' || task.status === 'assigned' ? 'warning' : task.status === 'failed' ? 'danger' : 'muted'}
            <button class="entity-list-item" onclick={() => nav('task', task.id, task)}>
              <Badge value={task.status ?? 'backlog'} variant={statusColor} />
              <div class="entity-list-main">
                <span class="entity-list-title">{task.title ?? entityName('task', task.id)}</span>
                {#if task.spec_path}
                  <span class="entity-list-sub mono">{task.spec_path.split('/').pop()}</span>
                {/if}
              </div>
              {#if task.priority}
                <Badge value={task.priority} variant={task.priority === 'high' || task.priority === 'critical' ? 'danger' : task.priority === 'low' ? 'muted' : 'warning'} />
              {/if}
              <span class="entity-list-time muted">{rel(task.created_at)}</span>
            </button>
          {/each}
        </div>
      {/if}

    {:else if activeTab === 'my-mrs'}
      {#if myMrs.length === 0}
        <EmptyState title="No merge requests" description="You haven't authored any merge requests." />
      {:else}
        <div class="entity-list">
          {#each myMrs as mr}
            {@const statusColor = mr.status === 'merged' ? 'success' : mr.status === 'open' ? 'info' : mr.status === 'closed' ? 'danger' : 'muted'}
            <button class="entity-list-item" onclick={() => nav('mr', mr.id, mr)}>
              <Badge value={mr.status ?? 'open'} variant={statusColor} />
              <div class="entity-list-main">
                <span class="entity-list-title">{mr.title ?? entityName('mr', mr.id)}</span>
                {#if mr.source_branch}
                  <span class="entity-list-sub mono">{mr.source_branch} → {mr.target_branch ?? 'main'}</span>
                {/if}
              </div>
              {#if mr.diff_stats}
                <span class="entity-list-diff">
                  <span class="diff-ins">+{mr.diff_stats.insertions ?? 0}</span>
                  <span class="diff-del">-{mr.diff_stats.deletions ?? 0}</span>
                </span>
              {/if}
              <span class="entity-list-time muted">{rel(mr.created_at)}</span>
            </button>
          {/each}
        </div>
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

    {:else if activeTab === 'ledger'}
      <!-- Judgment Ledger: chronological log of human judgment decisions -->
      <!-- Sourced from GET /api/v1/users/me/judgments -->
      {#if judgments.length === 0}
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
        <p class="prefs-desc">{$t('user_profile.prefs_desc')}</p>
        <div class="prefs-list">
          {#each NOTIF_TYPES as nt}
            <label class="pref-row">
              <input
                type="checkbox"
                class="pref-checkbox"
                bind:checked={notifPrefs[nt.id]}
                aria-label={$t('user_profile.enable_notification', { values: { label: nt.label } })}
              />
              <span class="pref-label">{$t(nt.labelKey)}</span>
            </label>
          {/each}
        </div>
        <div class="prefs-actions">
          <button class="btn-primary" onclick={savePrefs} disabled={prefsSaving} aria-busy={prefsSaving}>
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
            <div class="notif-item" class:unread={!notif.read}>
              <div class="notif-top">
                <Badge variant={notifColor(notif.notification_type)} value={notif.notification_type ?? 'info'} />
                <span class="notif-time muted">{rel(notif.created_at)}</span>
                {#if !notif.read}
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

  /* Entity list (agents, tasks, MRs) */
  .entity-list { display: flex; flex-direction: column; gap: var(--space-2); }
  .entity-list-item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    transition: border-color var(--transition-fast), background var(--transition-fast);
    text-align: left;
    width: 100%;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    color: var(--color-text);
  }
  .entity-list-item:hover {
    border-color: var(--color-border-strong);
    background: color-mix(in srgb, var(--color-surface-elevated) 80%, var(--color-focus));
  }
  .entity-list-item:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }
  .entity-list-main { flex: 1; display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .entity-list-title { font-weight: 500; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .entity-list-sub { font-size: var(--text-xs); color: var(--color-text-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .entity-list-time { font-size: var(--text-xs); flex-shrink: 0; }
  .entity-list-diff { display: flex; gap: var(--space-1); font-size: var(--text-xs); font-family: var(--font-mono); flex-shrink: 0; }
  .diff-ins { color: var(--color-success); }
  .diff-del { color: var(--color-danger); }

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
