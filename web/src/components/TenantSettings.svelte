<script>
  /**
   * TenantSettings — full-page tenant administration (§10 of ui-navigation.md)
   *
   * URL: /all/settings
   * Only visible to tenant Admin role users.
   * Tabs: Users | Compute Targets | Budget | Audit | Health | Jobs
   *
   * Spec ref: ui-navigation.md §10
   *   "Tenant administration is accessed via a gear icon on the cross-workspace view header.
   *    Only visible to tenant Admin role users. Tabs: Users, Compute Targets, Budget, Audit, Health, Jobs."
   */
  import { untrack } from 'svelte';
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let {
    onBack = undefined,
  } = $props();

  const TABS = [
    { id: 'users',    labelKey: 'tenant_settings.tabs.users' },
    { id: 'compute',  labelKey: 'tenant_settings.tabs.compute' },
    { id: 'budget',   labelKey: 'tenant_settings.tabs.budget' },
    { id: 'audit',    labelKey: 'tenant_settings.tabs.audit' },
    { id: 'health',   labelKey: 'tenant_settings.tabs.health' },
    { id: 'jobs',     labelKey: 'tenant_settings.tabs.jobs' },
  ];

  let activeTab = $state('users');

  // ── Users ─────────────────────────────────────────────────────────────
  let currentUser = $state(null);
  let usersLoading = $state(false);
  let usersError = $state(null);

  // ── Compute Targets ───────────────────────────────────────────────────
  let computeTargets = $state([]);
  let computeLoading = $state(false);
  let computeError = $state(null);

  // ── Budget ────────────────────────────────────────────────────────────
  let budgetSummary = $state(null);
  let budgetLoading = $state(false);
  let budgetError = $state(null);

  // ── Audit ─────────────────────────────────────────────────────────────
  let auditEvents = $state([]);
  let auditLoading = $state(false);
  let auditError = $state(null);
  let auditFilterType = $state('');

  // ── Health ────────────────────────────────────────────────────────────
  let health = $state(null);
  let healthLoading = $state(false);
  let healthError = $state(null);

  // ── Jobs ──────────────────────────────────────────────────────────────
  let jobs = $state([]);
  let jobsLoading = $state(false);
  let jobsError = $state(null);
  let runningJob = $state(null);

  // ── Sorting (per-table) ───────────────────────────────────────────────
  let computeSortCol = $state('name');
  let computeSortDir = $state(1);
  let budgetSortCol = $state('workspace_name');
  let budgetSortDir = $state(1);
  let auditSortCol = $state('timestamp');
  let auditSortDir = $state(-1);
  let jobsSortCol = $state('name');
  let jobsSortDir = $state(1);

  function toggleSort(col, currentCol, currentDir, setCol, setDir) {
    if (col === currentCol) { setDir(currentDir * -1); }
    else { setCol(col); setDir(1); }
  }

  function sortedBy(arr, col, dir) {
    return [...arr].sort((a, b) => {
      const av = a[col] ?? '';
      const bv = b[col] ?? '';
      if (av < bv) return -1 * dir;
      if (av > bv) return 1 * dir;
      return 0;
    });
  }

  function sortArrow(col, activeCol, dir) {
    return col === activeCol ? (dir === 1 ? ' ↑' : ' ↓') : '';
  }

  // ── Data loading driven by tab ─────────────────────────────────────────
  $effect(() => {
    const tab = activeTab;

    if (tab === 'users') {
      if (untrack(() => !currentUser && !usersLoading)) loadUsers();
    }
    if (tab === 'compute') {
      if (untrack(() => computeTargets.length === 0 && !computeLoading)) loadCompute();
    }
    if (tab === 'budget') {
      if (untrack(() => !budgetSummary && !budgetLoading)) loadBudget();
    }
    if (tab === 'audit') {
      if (untrack(() => auditEvents.length === 0 && !auditLoading)) loadAudit();
    }
    if (tab === 'health') {
      if (untrack(() => !health && !healthLoading)) loadHealth();
    }
    if (tab === 'jobs') {
      if (untrack(() => jobs.length === 0 && !jobsLoading)) loadJobs();
    }
  });

  async function loadUsers() {
    usersLoading = true;
    usersError = null;
    try {
      currentUser = await api.me();
    } catch (e) {
      usersError = e?.message ?? 'Failed to load user info';
    } finally {
      usersLoading = false;
    }
  }

  async function loadCompute() {
    computeLoading = true;
    computeError = null;
    try {
      const data = await api.computeList();
      computeTargets = Array.isArray(data) ? data : (data?.items ?? []);
    } catch (e) {
      computeError = e?.message ?? 'Failed to load compute targets';
    } finally {
      computeLoading = false;
    }
  }

  async function loadBudget() {
    budgetLoading = true;
    budgetError = null;
    try {
      budgetSummary = await api.budgetSummary();
    } catch (e) {
      budgetError = e?.message ?? 'Failed to load budget summary';
    } finally {
      budgetLoading = false;
    }
  }

  async function loadAudit() {
    auditLoading = true;
    auditError = null;
    try {
      const params = auditFilterType ? { event_type: auditFilterType } : {};
      const data = await api.adminAudit(params);
      auditEvents = Array.isArray(data) ? data : (data?.items ?? []);
    } catch (e) {
      auditError = e?.message ?? 'Failed to load audit log';
    } finally {
      auditLoading = false;
    }
  }

  async function refreshAudit() {
    auditEvents = [];
    await loadAudit();
  }

  async function loadHealth() {
    healthLoading = true;
    healthError = null;
    try {
      health = await api.adminHealth();
    } catch (e) {
      healthError = e?.message ?? 'Failed to load health status';
    } finally {
      healthLoading = false;
    }
  }

  async function loadJobs() {
    jobsLoading = true;
    jobsError = null;
    try {
      const data = await api.adminJobs();
      jobs = Array.isArray(data) ? data : (data?.jobs ?? []);
    } catch (e) {
      jobsError = e?.message ?? 'Failed to load jobs';
    } finally {
      jobsLoading = false;
    }
  }

  async function runJob(jobName) {
    runningJob = jobName;
    try {
      await api.adminRunJob(jobName);
      showToast(`Job "${jobName}" triggered`, { type: 'success' });
      jobs = [];
      await loadJobs();
    } catch (e) {
      showToast(`Failed to run job: ${e?.message ?? 'Unknown error'}`, { type: 'error' });
    } finally {
      runningJob = null;
    }
  }

  // ── Tab keyboard navigation ────────────────────────────────────────────
  let tabListEl = $state(null);

  function onTabKeydown(e) {
    const tabs = tabListEl?.querySelectorAll('[role="tab"]');
    if (!tabs?.length) return;
    const arr = Array.from(tabs);
    const current = arr.indexOf(document.activeElement);
    if (e.key === 'ArrowRight') { e.preventDefault(); arr[(current + 1) % arr.length]?.focus(); }
    else if (e.key === 'ArrowLeft') { e.preventDefault(); arr[(current - 1 + arr.length) % arr.length]?.focus(); }
    else if (e.key === 'Home') { e.preventDefault(); arr[0]?.focus(); }
    else if (e.key === 'End') { e.preventDefault(); arr[arr.length - 1]?.focus(); }
  }
</script>

<div class="tenant-settings" data-testid="tenant-settings">
  <!-- Page header with back button -->
  <header class="settings-header">
    <button
      class="back-btn"
      onclick={() => onBack?.()}
      aria-label="Back to All Workspaces"
      data-testid="tenant-settings-back"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <path d="M19 12H5M12 5l-7 7 7 7"/>
      </svg>
    </button>
    <div class="header-text">
      <h1 class="settings-title">{$t('tenant_settings.title')}</h1>
      <p class="settings-subtitle">System-wide configuration — users, compute, budget, audit, health, and background jobs.</p>
    </div>
  </header>

  <!-- Tab bar -->
  <div
    class="tab-bar"
    role="tablist"
    aria-label="Tenant administration sections"
    tabindex="-1"
    bind:this={tabListEl}
    onkeydown={onTabKeydown}
    data-testid="tenant-settings-tabs"
  >
    {#each TABS as tab (tab.id)}
      <button
        class="tab-btn"
        class:active={activeTab === tab.id}
        role="tab"
        tabindex={activeTab === tab.id ? 0 : -1}
        aria-selected={activeTab === tab.id}
        aria-controls="tab-panel-{tab.id}"
        onclick={() => { activeTab = tab.id; }}
        data-testid="tenant-settings-tab-{tab.id}"
      >
        {$t(tab.labelKey)}
      </button>
    {/each}
  </div>

  <!-- Tab content -->
  <div class="tab-content">

    <!-- ── Users ──────────────────────────────────────────────────────── -->
    {#if activeTab === 'users'}
      <div id="tab-panel-users" role="tabpanel" aria-label="Users" class="tab-panel" data-testid="tenant-tab-users">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.users.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.users.desc')}</p>
        </div>

        {#if usersLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.users.loading')}</div>
        {:else if usersError}
          <div class="panel-error" role="alert">{usersError}</div>
        {:else if currentUser}
          <div class="info-card">
            <div class="info-row">
              <span class="info-label">{$t('tenant_settings.users.current_user')}</span>
              <span class="info-value">{currentUser.username ?? currentUser.name ?? currentUser.email ?? '—'}</span>
            </div>
            {#if currentUser.email}
              <div class="info-row">
                <span class="info-label">{$t('tenant_settings.users.email')}</span>
                <span class="info-value">{currentUser.email}</span>
              </div>
            {/if}
            {#if currentUser.role}
              <div class="info-row">
                <span class="info-label">{$t('tenant_settings.users.role')}</span>
                <span class="info-value role-badge">{currentUser.role}</span>
              </div>
            {/if}
            {#if currentUser.tenant_id}
              <div class="info-row">
                <span class="info-label">{$t('tenant_settings.users.tenant_id')}</span>
                <span class="info-value mono">{currentUser.tenant_id}</span>
              </div>
            {/if}
          </div>
          <div class="panel-note">
            <p>{$t('tenant_settings.users.provisioning_note')}</p>
          </div>
        {:else}
          <div class="panel-empty">{$t('tenant_settings.users.no_user_info')}</div>
        {/if}
      </div>

    <!-- ── Compute Targets ────────────────────────────────────────────── -->
    {:else if activeTab === 'compute'}
      <div id="tab-panel-compute" role="tabpanel" aria-label="Compute Targets" class="tab-panel" data-testid="tenant-tab-compute">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.compute.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.compute.desc')}</p>
        </div>

        {#if computeLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.compute.loading')}</div>
        {:else if computeError}
          <div class="panel-error" role="alert">{computeError}</div>
        {:else if computeTargets.length === 0}
          <div class="panel-empty">{$t('tenant_settings.compute.empty')}</div>
        {:else}
          <table class="data-table" data-testid="compute-targets-table">
            <thead>
              <tr>
                <th scope="col" aria-sort={computeSortCol === 'name' ? (computeSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('name', computeSortCol, computeSortDir, v => computeSortCol = v, v => computeSortDir = v)}>{$t('tenant_settings.compute.col_name')}{sortArrow('name', computeSortCol, computeSortDir)}</button></th>
                <th scope="col" aria-sort={computeSortCol === 'kind' ? (computeSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('kind', computeSortCol, computeSortDir, v => computeSortCol = v, v => computeSortDir = v)}>{$t('tenant_settings.compute.col_kind')}{sortArrow('kind', computeSortCol, computeSortDir)}</button></th>
                <th scope="col" aria-sort={computeSortCol === 'status' ? (computeSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('status', computeSortCol, computeSortDir, v => computeSortCol = v, v => computeSortDir = v)}>{$t('tenant_settings.compute.col_status')}{sortArrow('status', computeSortCol, computeSortDir)}</button></th>
                <th scope="col" aria-sort={computeSortCol === 'capacity' ? (computeSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('capacity', computeSortCol, computeSortDir, v => computeSortCol = v, v => computeSortDir = v)}>{$t('tenant_settings.compute.col_capacity')}{sortArrow('capacity', computeSortCol, computeSortDir)}</button></th>
              </tr>
            </thead>
            <tbody>
              {#each sortedBy(computeTargets, computeSortCol, computeSortDir) as ct (ct.id ?? ct.name)}
                <tr>
                  <td class="td-name">{ct.name ?? '—'}</td>
                  <td>{ct.kind ?? ct.type ?? '—'}</td>
                  <td>
                    <span class="status-pill" class:status-ok={ct.status === 'healthy' || ct.status === 'active'} class:status-warn={ct.status === 'degraded'} class:status-err={ct.status === 'error' || ct.status === 'offline'}>
                      {ct.status ?? '—'}
                    </span>
                  </td>
                  <td>{ct.capacity ?? ct.max_agents ?? '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

    <!-- ── Budget ─────────────────────────────────────────────────────── -->
    {:else if activeTab === 'budget'}
      <div id="tab-panel-budget" role="tabpanel" aria-label="Budget" class="tab-panel" data-testid="tenant-tab-budget">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.budget.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.budget.desc')}</p>
        </div>

        {#if budgetLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.budget.loading')}</div>
        {:else if budgetError}
          <div class="panel-error" role="alert">{budgetError}</div>
        {:else if !budgetSummary}
          <div class="panel-empty">{$t('tenant_settings.budget.empty')}</div>
        {:else}
          <div class="budget-grid">
            {#if budgetSummary.total_credits != null}
              <div class="budget-card">
                <span class="budget-label">{$t('tenant_settings.budget.total_credits')}</span>
                <span class="budget-value">{budgetSummary.total_credits.toLocaleString()}</span>
              </div>
            {/if}
            {#if budgetSummary.used_credits != null}
              <div class="budget-card">
                <span class="budget-label">{$t('tenant_settings.budget.used_credits')}</span>
                <span class="budget-value">{budgetSummary.used_credits.toLocaleString()}</span>
              </div>
            {/if}
            {#if budgetSummary.total_credits && budgetSummary.used_credits != null}
              {@const pct = Math.round((budgetSummary.used_credits / budgetSummary.total_credits) * 100)}
              <div class="budget-card">
                <span class="budget-label">{$t('tenant_settings.budget.usage')}</span>
                <span class="budget-value" class:danger={pct > 90} class:warn={pct > 70 && pct <= 90}>{pct}%</span>
              </div>
            {/if}
            {#if budgetSummary.remaining_credits != null}
              <div class="budget-card">
                <span class="budget-label">{$t('tenant_settings.budget.remaining')}</span>
                <span class="budget-value">{budgetSummary.remaining_credits.toLocaleString()}</span>
              </div>
            {/if}
          </div>
          {#if budgetSummary.workspace_breakdown}
            <h3 class="sub-heading">{$t('tenant_settings.budget.per_workspace')}</h3>
            <table class="data-table" data-testid="budget-breakdown-table">
              <thead>
                <tr>
                  <th scope="col" aria-sort={budgetSortCol === 'workspace_name' ? (budgetSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('workspace_name', budgetSortCol, budgetSortDir, v => budgetSortCol = v, v => budgetSortDir = v)}>{$t('tenant_settings.budget.col_workspace')}{sortArrow('workspace_name', budgetSortCol, budgetSortDir)}</button></th>
                  <th scope="col" aria-sort={budgetSortCol === 'allocated' ? (budgetSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('allocated', budgetSortCol, budgetSortDir, v => budgetSortCol = v, v => budgetSortDir = v)}>{$t('tenant_settings.budget.col_allocated')}{sortArrow('allocated', budgetSortCol, budgetSortDir)}</button></th>
                  <th scope="col" aria-sort={budgetSortCol === 'used' ? (budgetSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('used', budgetSortCol, budgetSortDir, v => budgetSortCol = v, v => budgetSortDir = v)}>{$t('tenant_settings.budget.col_used')}{sortArrow('used', budgetSortCol, budgetSortDir)}</button></th>
                  <th scope="col" aria-sort={budgetSortCol === 'pct' ? (budgetSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('pct', budgetSortCol, budgetSortDir, v => budgetSortCol = v, v => budgetSortDir = v)}>{$t('tenant_settings.budget.col_usage_pct')}{sortArrow('pct', budgetSortCol, budgetSortDir)}</button></th>
                </tr>
              </thead>
              <tbody>
                {#each sortedBy(budgetSummary.workspace_breakdown, budgetSortCol, budgetSortDir) as row (row.workspace_id ?? row.workspace_name)}
                  <tr>
                    <td>{row.workspace_name ?? row.workspace_id ?? '—'}</td>
                    <td>{row.allocated ?? '—'}</td>
                    <td>{row.used ?? '—'}</td>
                    <td>{row.pct != null ? row.pct + '%' : '—'}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        {/if}
      </div>

    <!-- ── Audit ──────────────────────────────────────────────────────── -->
    {:else if activeTab === 'audit'}
      <div id="tab-panel-audit" role="tabpanel" aria-label="Audit" class="tab-panel" data-testid="tenant-tab-audit">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.audit.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.audit.desc')}</p>
        </div>

        <div class="filter-bar" data-testid="audit-filter-bar">
          <label for="audit-filter-type" class="filter-label">{$t('tenant_settings.audit.event_type')}</label>
          <select
            id="audit-filter-type"
            class="filter-select"
            bind:value={auditFilterType}
            onchange={refreshAudit}
          >
            <option value="">{$t('tenant_settings.audit.all_events')}</option>
            <option value="tenant_created">{$t('tenant_settings.audit_event_types.tenant_created')}</option>
            <option value="tenant_updated">{$t('tenant_settings.audit_event_types.tenant_updated')}</option>
            <option value="user_role_changed">{$t('tenant_settings.audit_event_types.user_role_changed')}</option>
            <option value="compute_target_added">{$t('tenant_settings.audit_event_types.compute_target_added')}</option>
            <option value="budget_updated">{$t('tenant_settings.audit_event_types.budget_updated')}</option>
            <option value="agent_killed">{$t('tenant_settings.audit_event_types.agent_killed')}</option>
            <option value="snapshot_created">{$t('tenant_settings.audit_event_types.snapshot_created')}</option>
            <option value="job_run">{$t('tenant_settings.audit_event_types.job_run')}</option>
          </select>
          <button class="refresh-btn" onclick={refreshAudit} aria-label={$t('tenant_settings.refresh')} data-testid="audit-refresh">
            {$t('tenant_settings.refresh')}
          </button>
        </div>

        {#if auditLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.audit.loading')}</div>
        {:else if auditError}
          <div class="panel-error" role="alert">{auditError}</div>
        {:else if auditEvents.length === 0}
          <div class="panel-empty">{$t('tenant_settings.audit_empty')}</div>
        {:else}
          <table class="data-table" data-testid="audit-events-table">
            <thead>
              <tr>
                <th scope="col" aria-sort={auditSortCol === 'timestamp' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('timestamp', auditSortCol, auditSortDir, v => auditSortCol = v, v => auditSortDir = v)}>{$t('tenant_settings.audit_col_time')}{sortArrow('timestamp', auditSortCol, auditSortDir)}</button></th>
                <th scope="col" aria-sort={auditSortCol === 'event_type' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('event_type', auditSortCol, auditSortDir, v => auditSortCol = v, v => auditSortDir = v)}>{$t('tenant_settings.audit_col_event')}{sortArrow('event_type', auditSortCol, auditSortDir)}</button></th>
                <th scope="col" aria-sort={auditSortCol === 'actor' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('actor', auditSortCol, auditSortDir, v => auditSortCol = v, v => auditSortDir = v)}>{$t('tenant_settings.audit_col_actor')}{sortArrow('actor', auditSortCol, auditSortDir)}</button></th>
                <th scope="col" aria-sort={auditSortCol === 'detail' ? (auditSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('detail', auditSortCol, auditSortDir, v => auditSortCol = v, v => auditSortDir = v)}>{$t('tenant_settings.audit_col_details')}{sortArrow('detail', auditSortCol, auditSortDir)}</button></th>
              </tr>
            </thead>
            <tbody>
              {#each sortedBy(auditEvents, auditSortCol, auditSortDir) as ev (ev.id ?? ev.timestamp)}
                <tr>
                  <td class="td-mono">{ev.timestamp ? new Date(ev.timestamp).toLocaleString() : '—'}</td>
                  <td><span class="event-type">{ev.event_type ?? ev.kind ?? '—'}</span></td>
                  <td>{ev.actor ?? ev.user ?? '—'}</td>
                  <td class="td-detail">{ev.detail ?? ev.message ?? '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>

    <!-- ── Health ─────────────────────────────────────────────────────── -->
    {:else if activeTab === 'health'}
      <div id="tab-panel-health" role="tabpanel" aria-label="Health" class="tab-panel" data-testid="tenant-tab-health">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.health.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.health.subtitle')}</p>
        </div>

        {#if healthLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.health.loading')}</div>
        {:else if healthError}
          <div class="panel-error" role="alert">{healthError}</div>
        {:else if !health}
          <div class="panel-empty">{$t('tenant_settings.health.empty')}</div>
        {:else}
          <div class="health-grid" data-testid="health-grid">
            {#each Object.entries(health) as [component, status] (component)}
              {@const ok = status === 'ok' || status === 'healthy' || status === true}
              {@const degraded = status === 'degraded' || status === 'warn'}
              <div class="health-card" class:health-ok={ok} class:health-warn={degraded} class:health-err={!ok && !degraded}>
                <span class="health-dot" aria-hidden="true"></span>
                <span class="health-component">{component}</span>
                <span class="health-status">{typeof status === 'boolean' ? (status ? 'ok' : 'error') : (status ?? '—')}</span>
              </div>
            {/each}
          </div>
        {/if}
      </div>

    <!-- ── Jobs ───────────────────────────────────────────────────────── -->
    {:else if activeTab === 'jobs'}
      <div id="tab-panel-jobs" role="tabpanel" aria-label="Jobs" class="tab-panel" data-testid="tenant-tab-jobs">
        <div class="panel-header">
          <h2 class="panel-title">{$t('tenant_settings.jobs.title')}</h2>
          <p class="panel-desc">{$t('tenant_settings.jobs.subtitle')}</p>
        </div>

        {#if jobsLoading}
          <div class="panel-loading" aria-live="polite">{$t('tenant_settings.jobs.loading')}</div>
        {:else if jobsError}
          <div class="panel-error" role="alert">{jobsError}</div>
        {:else if jobs.length === 0}
          <div class="panel-empty">{$t('tenant_settings.jobs.empty')}</div>
        {:else}
          <table class="data-table" data-testid="jobs-table">
            <thead>
              <tr>
                <th scope="col" aria-sort={jobsSortCol === 'name' ? (jobsSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('name', jobsSortCol, jobsSortDir, v => jobsSortCol = v, v => jobsSortDir = v)}>{$t('tenant_settings.jobs.col_job')}{sortArrow('name', jobsSortCol, jobsSortDir)}</button></th>
                <th scope="col" aria-sort={jobsSortCol === 'schedule' ? (jobsSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('schedule', jobsSortCol, jobsSortDir, v => jobsSortCol = v, v => jobsSortDir = v)}>{$t('tenant_settings.jobs.col_schedule')}{sortArrow('schedule', jobsSortCol, jobsSortDir)}</button></th>
                <th scope="col" aria-sort={jobsSortCol === 'last_run' ? (jobsSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('last_run', jobsSortCol, jobsSortDir, v => jobsSortCol = v, v => jobsSortDir = v)}>{$t('tenant_settings.jobs.col_last_run')}{sortArrow('last_run', jobsSortCol, jobsSortDir)}</button></th>
                <th scope="col" aria-sort={jobsSortCol === 'status' ? (jobsSortDir === 1 ? 'ascending' : 'descending') : 'none'}><button class="sort-btn" onclick={() => toggleSort('status', jobsSortCol, jobsSortDir, v => jobsSortCol = v, v => jobsSortDir = v)}>{$t('tenant_settings.jobs.col_status')}{sortArrow('status', jobsSortCol, jobsSortDir)}</button></th>
                <th scope="col">{$t('tenant_settings.audit.col_action')}</th>
              </tr>
            </thead>
            <tbody>
              {#each sortedBy(jobs, jobsSortCol, jobsSortDir) as job (job.name ?? job.id)}
                <tr>
                  <td class="td-name">{job.name ?? job.id ?? '—'}</td>
                  <td class="td-mono">{job.schedule ?? '—'}</td>
                  <td class="td-mono">{job.last_run ? new Date(job.last_run).toLocaleString() : '—'}</td>
                  <td>
                    <span class="status-pill" class:status-ok={job.status === 'ok' || job.status === 'success'} class:status-warn={job.status === 'running'} class:status-err={job.status === 'error' || job.status === 'failed'}>
                      {job.status ?? '—'}
                    </span>
                  </td>
                  <td>
                    <button
                      class="run-btn"
                      onclick={() => runJob(job.name ?? job.id)}
                      disabled={runningJob === (job.name ?? job.id)}
                      aria-label="Run job {job.name ?? job.id}"
                      data-testid="run-job-{job.name ?? job.id}"
                    >
                      {runningJob === (job.name ?? job.id) ? $t('tenant_settings.jobs.running') : $t('tenant_settings.jobs.run_now')}
                    </button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    {/if}

  </div>
</div>

<style>
  .tenant-settings {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    min-height: 0;
  }

  /* ── Header ────────────────────────────────────────────────────────────── */
  .settings-header {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-6) var(--space-8);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
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
    color: var(--color-text-secondary);
    cursor: pointer;
    flex-shrink: 0;
    transition: color var(--transition-fast), background var(--transition-fast);
  }

  .back-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .back-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .header-text { min-width: 0; }

  .settings-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0 0 var(--space-1) 0;
  }

  .settings-subtitle {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  /* ── Tab bar ────────────────────────────────────────────────────────────── */
  .tab-bar {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    flex-shrink: 0;
    padding: 0 var(--space-8);
    overflow-x: auto;
  }

  .tab-btn {
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    white-space: nowrap;
    transition: color var(--transition-fast), border-color var(--transition-fast);
    margin-bottom: -1px;
  }

  .tab-btn:hover {
    color: var(--color-text-secondary);
  }

  .tab-btn.active {
    color: var(--color-primary);
    border-bottom-color: var(--color-primary);
  }

  .tab-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  }

  /* ── Tab content ────────────────────────────────────────────────────────── */
  .tab-content {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }

  .tab-panel {
    padding: var(--space-6) var(--space-8);
    max-width: 900px;
  }

  /* ── Panel header ────────────────────────────────────────────────────────── */
  .panel-header {
    margin-bottom: var(--space-6);
  }

  .panel-title {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-1) 0;
  }

  .panel-desc {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  .panel-loading,
  .panel-empty {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    padding: var(--space-4) 0;
  }

  .panel-error {
    font-size: var(--text-sm);
    color: var(--color-danger);
    background: color-mix(in srgb, var(--color-danger) 8%, transparent);
    border-left: 3px solid var(--color-danger);
    padding: var(--space-3) var(--space-4);
    border-radius: 0 var(--radius-sm) var(--radius-sm) 0;
    margin-bottom: var(--space-4);
  }

  .panel-note {
    margin-top: var(--space-4);
    padding: var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
  }

  .panel-note p { margin: 0; }

  /* ── Info card (Users tab) ────────────────────────────────────────────── */
  .info-card {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    max-width: 480px;
  }

  .info-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .info-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    width: 100px;
    flex-shrink: 0;
  }

  .info-value {
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .info-value.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    word-break: break-all;
  }

  .role-badge {
    display: inline-block;
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    color: var(--color-primary);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-2);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  /* ── Data tables ─────────────────────────────────────────────────────── */
  .data-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .data-table th {
    padding: 0;
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    background: var(--color-surface-elevated);
    border-bottom: 1px solid var(--color-border);
  }

  .sort-btn {
    width: 100%;
    text-align: left;
    padding: var(--space-2) var(--space-4);
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    cursor: pointer;
    transition: color var(--transition-fast);
  }

  .sort-btn:hover { color: var(--color-text); }

  .sort-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .data-table td {
    padding: var(--space-3) var(--space-4);
    color: var(--color-text-secondary);
    border-bottom: 1px solid var(--color-border);
    vertical-align: middle;
  }

  .data-table tr:last-child td { border-bottom: none; }

  .data-table tr:hover td { background: var(--color-surface-elevated); }

  .td-name {
    font-weight: 500;
    color: var(--color-text);
  }

  .td-mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .td-detail {
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Status pills ────────────────────────────────────────────────────── */
  .status-pill {
    display: inline-block;
    padding: 2px var(--space-2);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 600;
    background: var(--color-border);
    color: var(--color-text-muted);
  }

  .status-pill.status-ok {
    background: color-mix(in srgb, var(--color-success) 15%, transparent);
    color: var(--color-success);
  }

  .status-pill.status-warn {
    background: color-mix(in srgb, var(--color-warning) 15%, transparent);
    color: var(--color-warning);
  }

  .status-pill.status-err {
    background: color-mix(in srgb, var(--color-danger) 15%, transparent);
    color: var(--color-danger);
  }

  /* ── Event type badge ────────────────────────────────────────────────── */
  .event-type {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--color-border);
    color: var(--color-text-secondary);
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
  }

  /* ── Budget grid ─────────────────────────────────────────────────────── */
  .budget-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-4);
    margin-bottom: var(--space-6);
  }

  .budget-card {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .budget-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 600;
  }

  .budget-value {
    font-size: var(--text-xl);
    font-weight: 700;
    color: var(--color-text);
  }

  .budget-value.danger { color: var(--color-danger); }
  .budget-value.warn { color: var(--color-warning); }

  .sub-heading {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text-secondary);
    margin: 0 0 var(--space-3) 0;
  }

  /* ── Health grid ─────────────────────────────────────────────────────── */
  .health-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-3);
  }

  .health-card {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    font-size: var(--text-sm);
  }

  .health-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--color-text-muted);
    flex-shrink: 0;
  }

  .health-card.health-ok .health-dot { background: var(--color-success); }
  .health-card.health-warn .health-dot { background: var(--color-warning); }
  .health-card.health-err .health-dot { background: var(--color-danger); }

  .health-component {
    flex: 1;
    color: var(--color-text);
    font-weight: 500;
  }

  .health-status {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* ── Audit filter bar ────────────────────────────────────────────────── */
  .filter-bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-4);
    flex-wrap: wrap;
  }

  .filter-label {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    white-space: nowrap;
  }

  .filter-select {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .filter-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .refresh-btn {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .refresh-btn:hover { background: var(--color-surface-elevated); }

  .refresh-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Run job button ──────────────────────────────────────────────────── */
  .run-btn {
    padding: var(--space-1) var(--space-3);
    background: transparent;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .run-btn:hover:not(:disabled) {
    background: var(--color-surface-elevated);
    border-color: var(--color-text-muted);
  }

  .run-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .run-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* ── Responsive ──────────────────────────────────────────────────────── */
  @media (max-width: 768px) {
    .settings-header { padding: var(--space-4); }
    .tab-bar { padding: 0 var(--space-3); }
    .tab-panel { padding: var(--space-4); }
    .budget-grid { grid-template-columns: repeat(2, 1fr); }
    .td-detail { max-width: 150px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .back-btn, .tab-btn, .refresh-btn, .run-btn { transition: none; }
  }
</style>
