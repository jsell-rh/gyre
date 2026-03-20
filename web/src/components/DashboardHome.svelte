<script>
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
    try {
      const [ag, ts, mq, ac] = await Promise.all([
        fetch('/api/v1/agents').then(r => r.ok ? r.json() : []),
        fetch('/api/v1/tasks').then(r => r.ok ? r.json() : []),
        fetch('/api/v1/merge-queue').then(r => r.ok ? r.json() : []),
        fetch('/api/v1/activity?limit=10').then(r => r.ok ? r.json() : []),
      ]);
      agents   = ag?.agents   ?? ag ?? [];
      tasks    = ts?.tasks    ?? ts ?? [];
      queue    = mq?.items    ?? mq ?? [];
      activity = ac?.events   ?? ac ?? [];
    } catch {
      // fallback: leave as empty
    }
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

<Modal bind:open={showNewProject} title="New Project">
  <div class="qa-form">
    <label class="qa-label">Name
      <input class="qa-input" bind:value={qaName} placeholder="my-project" />
    </label>
    <label class="qa-label">Description
      <input class="qa-input" bind:value={qaDesc} placeholder="Optional description" />
    </label>
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (showNewProject = false)}>Cancel</Button>
    <Button variant="primary" onclick={quickCreateProject} disabled={qaCreating || !qaName.trim()}>
      {qaCreating ? 'Creating…' : 'Create Project'}
    </Button>
  {/snippet}
</Modal>

<Modal bind:open={showNewTask} title="New Task">
  <div class="qa-form">
    <label class="qa-label">Title
      <input class="qa-input" bind:value={qaTaskTitle} placeholder="Task title" />
    </label>
    <label class="qa-label">Priority
      <select class="qa-input" bind:value={qaTaskPriority}>
        <option value="Critical">Critical</option>
        <option value="High">High</option>
        <option value="Medium">Medium</option>
        <option value="Low">Low</option>
      </select>
    </label>
  </div>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (showNewTask = false)}>Cancel</Button>
    <Button variant="primary" onclick={quickCreateTask} disabled={qaTaskCreating || !qaTaskTitle.trim()}>
      {qaTaskCreating ? 'Creating…' : 'Create Task'}
    </Button>
  {/snippet}
</Modal>

<div class="dashboard">
  <!-- Metric cards -->
  <section class="metrics">
    <button class="metric-card" onclick={() => onnavigate?.('agents')}>
      <div class="metric-label">Active Agents</div>
      {#if loading}
        <Skeleton height="2rem" width="3rem" />
      {:else}
        <div class="metric-value">{activeAgents.length}</div>
        <div class="metric-sub">{agents.length} total</div>
      {/if}
    </button>

    <button class="metric-card" onclick={() => onnavigate?.('tasks')}>
      <div class="metric-label">Open Tasks</div>
      {#if loading}
        <Skeleton height="2rem" width="3rem" />
      {:else}
        <div class="metric-value">{openTasks.length}</div>
        <div class="metric-sub">{tasks.length} total</div>
      {/if}
    </button>

    <button class="metric-card" onclick={() => onnavigate?.('projects')}>
      <div class="metric-label">Pending MRs</div>
      {#if loading}
        <Skeleton height="2rem" width="3rem" />
      {:else}
        <div class="metric-value">{pendingMrs.length}</div>
        <div class="metric-sub">open for review</div>
      {/if}
    </button>

    <button class="metric-card" onclick={() => onnavigate?.('merge-queue')}>
      <div class="metric-label">Queue Depth</div>
      {#if loading}
        <Skeleton height="2rem" width="3rem" />
      {:else}
        <div class="metric-value">{queueDepth}</div>
        <div class="metric-sub">{queue.length} total entries</div>
      {/if}
    </button>
  </section>

  <!-- Quick actions -->
  <section class="quick-actions">
    <Button variant="secondary" onclick={() => (showNewProject = true)}>+ New Project</Button>
    <Button variant="secondary" onclick={() => (showNewTask = true)}>+ New Task</Button>
    <Button variant="secondary" onclick={seedDemoData} disabled={seedLoading}>
      {seedLoading ? 'Seeding…' : '⚡ Seed Demo Data'}
    </Button>
  </section>

  <div class="dashboard-grid">
    <!-- Agent health grid -->
    <Card>
      {#snippet header()}
        <span>Agent Health</span>
        <button class="view-all" onclick={() => onnavigate?.('agents')}>View all</button>
      {/snippet}
      {#if loading}
        <div class="agent-grid">
          {#each Array(8) as _}
            <Skeleton height="2.5rem" radius="var(--radius)" />
          {/each}
        </div>
      {:else if agents.length === 0}
        <EmptyState title="No agents" description="No agents found." />
      {:else}
        <div class="agent-grid">
          {#each agents as agent}
            <div class="agent-chip" title="{agent.name}: {agent.status}">
              <span
                class="agent-dot"
                style="background: {agentStatusColor(agent.status)}"
              ></span>
              <span class="agent-name">{agent.name}</span>
            </div>
          {/each}
        </div>
      {/if}
    </Card>

    <!-- Recent activity -->
    <Card>
      {#snippet header()}
        <span>Recent Activity</span>
        <button class="view-all" onclick={() => onnavigate?.('activity')}>View all</button>
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
        <EmptyState title="No activity yet" description="Events will appear here." />
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
          <span>Merge Queue</span>
          <button class="view-all" onclick={() => onnavigate?.('merge-queue')}>View all</button>
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

  .qa-input:focus { outline: none; border-color: var(--color-primary); }

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
    gap: var(--space-4);
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
    background: rgba(0, 102, 204, 0.1);
    border: 1px solid rgba(0, 102, 204, 0.3);
    border-radius: var(--radius);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    font-weight: 600;
    color: var(--color-link);
  }

  .queue-item.processing {
    background: rgba(245, 146, 27, 0.1);
    border-color: rgba(245, 146, 27, 0.3);
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

  @media (max-width: 900px) {
    .metrics { grid-template-columns: repeat(2, 1fr); }
    .dashboard-grid { grid-template-columns: 1fr; }
  }
</style>
