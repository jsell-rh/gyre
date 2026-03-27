<script>
  import { t } from 'svelte-i18n';
  import Card from '../lib/Card.svelte';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Modal from '../lib/Modal.svelte';
  import Button from '../lib/Button.svelte';
  import { api } from '../lib/api.js';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let { wsStore = null, onnavigate = undefined } = $props();

  let agents = $state([]);
  let tasks = $state([]);
  let mrs = $state([]);
  let queue = $state([]);
  let activity = $state([]);
  let loading = $state(true);

  async function fetchAll() {
    loading = true;
    const [agR, tsR, mqR, acR] = await Promise.allSettled([
      api.agents(),
      api.tasks(),
      api.mergeQueue(),
      api.activity(10),
    ]);
    if (agR.status === 'fulfilled') { const r = agR.value; agents = Array.isArray(r?.agents) ? r.agents : Array.isArray(r) ? r : []; }
    if (tsR.status === 'fulfilled') { const r = tsR.value; tasks = Array.isArray(r?.tasks) ? r.tasks : Array.isArray(r) ? r : []; }
    if (mqR.status === 'fulfilled') { const r = mqR.value; queue = Array.isArray(r?.items) ? r.items : Array.isArray(r) ? r : []; }
    if (acR.status === 'fulfilled') { const r = acR.value; activity = Array.isArray(r?.events) ? r.events : Array.isArray(r) ? r : []; }
    loading = false;
  }

  $effect(() => {
    fetchAll();
  });

  // Quick action modals
  let showNewProject = $state(false);
  let qaName = $state('');
  let qaDesc = $state('');
  let qaCreating = $state(false);

  let showNewTask = $state(false);
  let qaTaskTitle = $state('');
  let qaTaskPriority = $state('Medium');
  let qaTaskCreating = $state(false);

  let seedLoading = $state(false);

  async function quickCreateProject() {
    if (!qaName.trim()) return;
    qaCreating = true;
    try {
      await api.createProject({ name: qaName.trim(), description: qaDesc.trim() || undefined });
      toastSuccess('Project created');
      showNewProject = false;
      qaName = ''; qaDesc = '';
    } catch (e) {
      toastError(e.message);
    }
    qaCreating = false;
  }

  async function quickCreateTask() {
    if (!qaTaskTitle.trim()) return;
    qaTaskCreating = true;
    try {
      await api.createTask({ title: qaTaskTitle.trim(), priority: qaTaskPriority, status: 'Backlog' });
      toastSuccess('Task created');
      showNewTask = false;
      qaTaskTitle = ''; qaTaskPriority = 'Medium';
      fetchAll();
    } catch (e) {
      toastError(e.message);
    }
    qaTaskCreating = false;
  }

  async function seedDemoData() {
    seedLoading = true;
    try {
      await api.seedData();
      toastSuccess('Demo data seeded');
      fetchAll();
    } catch (e) {
      toastError(e.message);
    }
    seedLoading = false;
  }

  let activeAgents    = $derived(agents.filter(a => a.status === 'Active' || a.status === 'active'));
  let openTasks       = $derived(tasks.filter(t => t.status === 'InProgress' || t.status === 'in_progress' || t.status === 'Backlog' || t.status === 'backlog'));
  let pendingMrs      = $derived(mrs.filter(m => m.status === 'Open' || m.status === 'open'));
  let queueDepth      = $derived(queue.filter(q => q.status === 'Queued' || q.status === 'Processing').length);

  function relativeTime(ts) {
    if (!ts) return '';
    const diff = Date.now() - new Date(ts).getTime();
    const s = Math.floor(diff / 1000);
    if (s < 60) return `${s}s ago`;
    const m = Math.floor(s / 60);
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }

  function agentStatusColor(status) {
    const s = (status ?? '').toLowerCase();
    if (s === 'active') return 'var(--color-success)';
    if (s === 'blocked') return 'var(--color-blocked)';
    if (s === 'error' || s === 'dead') return 'var(--color-danger)';
    return 'var(--color-text-muted)';
  }
</script>

<Modal bind:open={showNewProject} title={$t('projects.new_project.title')}>
  <div class="qa-form">
    <label class="qa-label">{$t('projects.new_project.name_label')}
      <input class="qa-input" bind:value={qaName} placeholder={$t('projects.new_project.name_placeholder')} />
    </label>
    <label class="qa-label">{$t('projects.new_project.desc_label')}
      <input class="qa-input" bind:value={qaDesc} placeholder={$t('projects.new_project.desc_placeholder')} />
    </label>
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (showNewProject = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={quickCreateProject} disabled={qaCreating || !qaName.trim()}>
      {qaCreating ? $t('common.creating') : $t('common.create') + ' Project'}
    </Button>
  {/snippet}
</Modal>

<Modal bind:open={showNewTask} title={$t('tasks.new_task.title')}>
  <div class="qa-form">
    <label class="qa-label">{$t('tasks.new_task.title_label')}
      <input class="qa-input" bind:value={qaTaskTitle} placeholder={$t('tasks.new_task.title_placeholder')} />
    </label>
    <label class="qa-label">{$t('tasks.new_task.priority_label')}
      <select class="qa-input" bind:value={qaTaskPriority}>
        <option value="Critical">{$t('tasks.priority.critical')}</option>
        <option value="High">{$t('tasks.priority.high')}</option>
        <option value="Medium">{$t('tasks.priority.medium')}</option>
        <option value="Low">{$t('tasks.priority.low')}</option>
      </select>
    </label>
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (showNewTask = false)}>{$t('common.cancel')}</Button>
    <Button variant="primary" onclick={quickCreateTask} disabled={qaTaskCreating || !qaTaskTitle.trim()}>
      {qaTaskCreating ? $t('common.creating') : $t('common.create') + ' Task'}
    </Button>
  {/snippet}
</Modal>

<div class="dashboard" aria-busy={loading}>
  <span class="sr-only" aria-live="polite">{loading ? '' : 'Dashboard loaded'}</span>
  <!-- Page header -->
  <div class="page-header">
    <h1 class="page-title">{$t('dashboard.title')}</h1>
    <p class="page-desc">Platform overview and quick actions</p>
  </div>

  <!-- Metric cards -->
  <section class="metrics" aria-label="Dashboard metrics">
    <button class="metric-card" onclick={() => onnavigate?.('agents')} aria-label="View agents — {activeAgents.length} active of {agents.length} total">
      <div class="metric-label">{$t('dashboard.metrics.active_agents')}</div>
      {#if loading}
        <Skeleton height="2rem" width="3rem" />
      {:else}
        <div class="metric-value">{activeAgents.length}</div>
        <div class="metric-sub">{$t('dashboard.metrics.total', { values: { count: agents.length } })}</div>
      {/if}
    </button>

    <button class="metric-card" onclick={() => onnavigate?.('tasks')} aria-label="View tasks — {openTasks.length} open of {tasks.length} total">
      <div class="metric-label">{$t('dashboard.metrics.open_tasks')}</div>
      {#if loading}
        <Skeleton height="2rem" width="3rem" />
      {:else}
        <div class="metric-value">{openTasks.length}</div>
        <div class="metric-sub">{$t('dashboard.metrics.total', { values: { count: tasks.length } })}</div>
      {/if}
    </button>

    <button class="metric-card" onclick={() => onnavigate?.('projects')} aria-label="View merge requests — {pendingMrs.length} pending">
      <div class="metric-label">{$t('dashboard.metrics.pending_mrs')}</div>
      {#if loading}
        <Skeleton height="2rem" width="3rem" />
      {:else}
        <div class="metric-value">{pendingMrs.length}</div>
        <div class="metric-sub">{$t('dashboard.metrics.open_for_review')}</div>
      {/if}
    </button>

    <button class="metric-card" onclick={() => onnavigate?.('merge-queue')} aria-label="View merge queue — {queueDepth} queued of {queue.length} total">
      <div class="metric-label">{$t('dashboard.metrics.queue_depth')}</div>
      {#if loading}
        <Skeleton height="2rem" width="3rem" />
      {:else}
        <div class="metric-value">{queueDepth}</div>
        <div class="metric-sub">{$t('dashboard.metrics.total_entries', { values: { count: queue.length } })}</div>
      {/if}
    </button>
  </section>

  <!-- Quick actions -->
  <section class="quick-actions" aria-label="Quick actions">
    <Button variant="secondary" onclick={() => (showNewProject = true)}>{$t('dashboard.quick_actions.new_project')}</Button>
    <Button variant="secondary" onclick={() => (showNewTask = true)}>{$t('dashboard.quick_actions.new_task')}</Button>
    <Button variant="secondary" onclick={seedDemoData} disabled={seedLoading}>
      {seedLoading ? $t('dashboard.quick_actions.seeding') : $t('dashboard.quick_actions.seed_demo')}
    </Button>
  </section>

  <div class="dashboard-grid">
    <!-- Agent health grid -->
    <Card>
      {#snippet header()}
        <span>{$t('dashboard.sections.agent_health')}</span>
        <button class="view-all" onclick={() => onnavigate?.('agents')} aria-label="{$t('dashboard.view_all')} agents">{$t('dashboard.view_all')}</button>
      {/snippet}
      {#if loading}
        <div class="agent-grid">
          {#each Array(8) as _}
            <Skeleton height="2.5rem" radius="var(--radius)" />
          {/each}
        </div>
      {:else if agents.length === 0}
        <EmptyState title={$t('dashboard.empty.no_agents')} description={$t('dashboard.empty.no_agents_desc')} />
      {:else}
        <div class="agent-grid">
          {#each agents as agent}
            <div class="agent-chip">
              <span
                class="agent-dot"
                style="background: {agentStatusColor(agent.status)}"
                aria-hidden="true"
              ></span>
              <span class="agent-name">{agent.name}</span>
              <span class="sr-only">, status: {agent.status}</span>
            </div>
          {/each}
        </div>
      {/if}
    </Card>

    <!-- Recent activity -->
    <Card>
      {#snippet header()}
        <span>{$t('dashboard.sections.recent_activity')}</span>
        <button class="view-all" onclick={() => onnavigate?.('activity')} aria-label="{$t('dashboard.view_all')} activity">{$t('dashboard.view_all')}</button>
      {/snippet}
      {#if loading}
        <div class="activity-list">
          {#each Array(5) as _}
            <div class="activity-skeleton">
              <Skeleton height="0.75rem" width="80px" />
              <Skeleton height="0.75rem" />
            </div>
          {/each}
        </div>
      {:else if activity.length === 0}
        <EmptyState title={$t('dashboard.empty.no_activity')} description={$t('dashboard.empty.no_activity_desc')} />
      {:else}
        <ul class="activity-list">
          {#each activity.slice(0, 10) as event}
            <li class="activity-item">
              <span class="activity-time">{relativeTime(event.timestamp)}</span>
              <span class="activity-msg">{event.message ?? event.event_type ?? 'Event'}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </Card>

    <!-- Merge queue status -->
    {#if queue.length > 0}
      <Card>
        {#snippet header()}
          <span>{$t('dashboard.sections.merge_queue')}</span>
          <button class="view-all" onclick={() => onnavigate?.('merge-queue')} aria-label="{$t('dashboard.view_all')} merge queue">{$t('dashboard.view_all')}</button>
        {/snippet}
        <div class="queue-bar">
          {#each queue.slice(0, 8) as item}
            <div
              class="queue-item"
              class:processing={item.status === 'Processing' || item.status === 'processing'}
              title="{item.title ?? item.id}: {item.status}"
            >
              <span class="queue-pos">{item.position ?? '#'}</span>
            </div>
          {/each}
        </div>
      </Card>
    {/if}
  </div>
</div>

<style>
  .dashboard {
    padding: var(--space-6);
    overflow-y: auto;
    height: 100%;
    max-width: var(--content-max-width);
  }

  /* Quick actions */
  .quick-actions {
    display: flex;
    gap: var(--space-3);
    margin-bottom: var(--space-6);
    flex-wrap: wrap;
  }

  /* Modal form helpers */
  .qa-form { display: flex; flex-direction: column; gap: var(--space-3); }

  .qa-label {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    font-weight: 500;
  }

  .qa-input {
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    transition: border-color var(--transition-fast);
  }

  .qa-input:focus:not(:focus-visible) { outline: none; }
  .qa-input:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }

  /* Page header */
  .page-header {
    margin-bottom: var(--space-2);
  }

  .page-title {
    font-family: var(--font-display);
    font-size: var(--text-2xl);
    font-weight: 600;
    color: var(--color-text);
    margin: 0 0 var(--space-1);
  }

  .page-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
  }

  /* Metric cards */
  .metrics {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: var(--space-4);
    margin-bottom: var(--space-6);
  }

  .metric-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-6);
    text-align: left;
    cursor: pointer;
    transition: border-color var(--transition-fast), background var(--transition-fast);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .metric-card:hover {
    border-color: var(--color-border-strong);
    background: var(--color-surface-elevated);
  }

  .metric-card:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .metric-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-secondary);
  }

  .metric-value {
    font-family: var(--font-display);
    font-size: var(--text-3xl);
    font-weight: 700;
    color: var(--color-text);
    line-height: 1;
    margin-top: var(--space-2);
  }

  .metric-sub {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin-top: var(--space-1);
  }

  /* Grid */
  .dashboard-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-6);
    align-items: start;
  }

  /* Agent grid */
  .agent-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: var(--space-2);
  }

  .agent-chip {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .agent-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .agent-name {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Activity */
  .activity-list {
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .activity-item {
    display: grid;
    grid-template-columns: 60px 1fr;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border);
    align-items: baseline;
  }

  .activity-item:last-child {
    border-bottom: none;
  }

  .activity-time {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .activity-msg {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .activity-skeleton {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border);
  }

  /* Queue bar */
  .queue-bar {
    display: flex;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .queue-item {
    width: 44px;
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-info) 30%, transparent);
    border-radius: var(--radius);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    font-weight: 600;
    color: var(--color-link);
  }

  .queue-item.processing {
    background: color-mix(in srgb, var(--color-warning) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-warning) 30%, transparent);
    color: var(--color-warning);
    animation: pulse 1.5s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  .queue-pos {
    font-size: var(--text-xs);
    font-weight: 700;
  }

  /* View all button */
  .view-all {
    background: transparent;
    border: none;
    font-size: var(--text-xs);
    color: var(--color-link);
    cursor: pointer;
    padding: 0;
    font-family: var(--font-body);
    transition: color var(--transition-fast);
  }

  .view-all:hover {
    color: var(--color-link-hover);
  }

  .view-all:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  @media (prefers-reduced-motion: reduce) {
    .queue-item.processing { animation: none; }
    .metric-card,
    .qa-input,
    .view-all { transition: none; }
  }

  @media (max-width: 900px) {
    .metrics { grid-template-columns: repeat(2, 1fr); }
    .dashboard-grid { grid-template-columns: 1fr; }
  }
</style>
