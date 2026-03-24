<script>
  import Card from '../lib/Card.svelte';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import { api } from '../lib/api.js';
  import { getContext } from 'svelte';

  const navigate = getContext('navigate');

  let agents = $state([]);
  let tasks = $state([]);
  let mrs = $state([]);
  let activity = $state([]);
  let loading = $state(true);

  async function fetchAll() {
    loading = true;
    const [agR, tsR, mrR, acR] = await Promise.allSettled([
      api.agents(),
      api.tasks(),
      api.mergeRequests({ status: 'open' }),
      api.activity(10),
    ]);
    if (agR.status === 'fulfilled') { const r = agR.value; agents = Array.isArray(r?.agents) ? r.agents : Array.isArray(r) ? r : []; }
    if (tsR.status === 'fulfilled') { const r = tsR.value; tasks = Array.isArray(r?.tasks) ? r.tasks : Array.isArray(r) ? r : []; }
    if (mrR.status === 'fulfilled') { const r = mrR.value; mrs = Array.isArray(r?.merge_requests) ? r.merge_requests : Array.isArray(r) ? r : []; }
    if (acR.status === 'fulfilled') { const r = acR.value; activity = Array.isArray(r?.events) ? r.events : Array.isArray(r) ? r : []; }
    loading = false;
  }

  $effect(() => { fetchAll(); });

  let activeAgents  = $derived(agents.filter(a => a.status === 'Active'  || a.status === 'active'));
  let openTasks     = $derived(tasks.filter(t  => t.status === 'in_progress' || t.status === 'InProgress' || t.status === 'backlog' || t.status === 'Backlog'));
  let openMrs       = $derived(mrs.filter(m   => m.status === 'Open'  || m.status === 'open'));
  let blockedTasks  = $derived(tasks.filter(t  => t.status === 'blocked' || t.status === 'Blocked'));

  function relativeTime(ts) {
    if (!ts) return '';
    const d = new Date(typeof ts === 'number' ? ts * 1000 : ts);
    const diff = (Date.now() - d.getTime()) / 1000;
    if (diff < 60)    return 'just now';
    if (diff < 3600)  return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  const EVENT_TYPE_COLORS = {
    RUN_STARTED:   'success',
    RUN_FINISHED:  'info',
    STATE_CHANGED: 'default',
    ERROR:         'danger',
    TOOL_CALL_START: 'default',
    TOOL_CALL_END:   'default',
  };
</script>

<div class="briefing">
  <div class="briefing-header">
    <h1 class="briefing-title">Daily Briefing</h1>
    <p class="briefing-subtitle">System snapshot as of {new Date().toLocaleTimeString()}</p>
  </div>

  {#if loading}
    <div class="metrics-grid">
      {#each [0,1,2,3] as _}
        <div class="metric-skeleton">
          <Skeleton width="60px" height="36px" />
          <Skeleton width="90px" height="14px" />
        </div>
      {/each}
    </div>
    <div class="section-skeleton">
      {#each [0,1,2,3,4] as _}
        <Skeleton width="100%" height="48px" />
      {/each}
    </div>
  {:else}
    <!-- Metric cards -->
    <div class="metrics-grid">
      <button class="metric-card" onclick={() => navigate?.('agents')} aria-label="Navigate to agents">
        <span class="metric-value">{activeAgents.length}</span>
        <span class="metric-label">Active Agents</span>
      </button>
      <button class="metric-card" onclick={() => navigate?.('tasks')} aria-label="Navigate to tasks">
        <span class="metric-value">{openTasks.length}</span>
        <span class="metric-label">Open Tasks</span>
      </button>
      <button class="metric-card" onclick={() => navigate?.('merge-queue')} aria-label="Navigate to merge queue">
        <span class="metric-value">{openMrs.length}</span>
        <span class="metric-label">Pending MRs</span>
      </button>
      <button class="metric-card metric-card--warn" aria-label="Blocked task count">
        <span class="metric-value">{blockedTasks.length}</span>
        <span class="metric-label">Blocked Tasks</span>
      </button>
    </div>

    <!-- Recent activity -->
    <section class="briefing-section" aria-labelledby="section-activity">
      <h2 class="section-title" id="section-activity">Recent Activity</h2>
      {#if activity.length === 0}
        <EmptyState title="No recent activity" description="Nothing has happened yet." />
      {:else}
        <ul class="activity-list" role="list">
          {#each activity as event}
            <li class="activity-item">
              <span
                class="activity-dot"
                style="background: var(--color-{EVENT_TYPE_COLORS[event.event_type] ?? 'text-muted'})"
                aria-hidden="true"
              ></span>
              <div class="activity-body">
                <p class="activity-desc">{event.description ?? event.event_type ?? 'Event'}</p>
                {#if event.agent_id}
                  <span class="activity-agent">agent:{event.agent_id.slice(0,8)}</span>
                {/if}
              </div>
              <span class="activity-time">{relativeTime(event.timestamp)}</span>
            </li>
          {/each}
        </ul>
        <button class="view-all-btn" onclick={() => navigate?.('activity')}>
          View all activity →
        </button>
      {/if}
    </section>

    <!-- Quick links -->
    <section class="briefing-section" aria-labelledby="section-links">
      <h2 class="section-title" id="section-links">Quick Links</h2>
      <div class="quick-links">
        {#each [
          { view: 'agents',      label: 'Agents',       icon: '🤖' },
          { view: 'tasks',       label: 'Tasks',        icon: '📋' },
          { view: 'merge-queue', label: 'Merge Queue',  icon: '⏱' },
          { view: 'analytics',   label: 'Analytics',    icon: '📊' },
          { view: 'specs',       label: 'Specs',        icon: '📄' },
          { view: 'audit',       label: 'Audit',        icon: '🔍' },
        ] as link}
          <button class="quick-link" onclick={() => navigate?.(link.view)} aria-label="Go to {link.label}">
            <span class="quick-link-icon" aria-hidden="true">{link.icon}</span>
            <span>{link.label}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}
</div>

<style>
  .briefing {
    padding: var(--space-6);
    max-width: 900px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-8);
  }

  .briefing-header {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .briefing-title {
    font-family: var(--font-display);
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .briefing-subtitle {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  /* Metric cards */
  .metrics-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--space-4);
  }

  .metric-skeleton {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .metric-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-5);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    cursor: pointer;
    text-align: left;
    transition: border-color var(--transition-fast), background var(--transition-fast);
    font-family: inherit;
  }

  .metric-card:hover {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 4%, var(--color-surface));
  }

  .metric-card--warn {
    cursor: default;
  }

  .metric-card--warn:hover {
    border-color: var(--color-border);
    background: var(--color-surface);
  }

  .metric-value {
    font-family: var(--font-display);
    font-size: 2rem;
    font-weight: 700;
    color: var(--color-text);
    line-height: 1;
  }

  .metric-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 500;
  }

  /* Activity section */
  .briefing-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }

  .section-skeleton {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .activity-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .activity-item {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border);
  }

  .activity-item:last-child {
    border-bottom: none;
  }

  .activity-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
    margin-top: 5px;
  }

  .activity-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .activity-desc {
    font-size: var(--text-sm);
    color: var(--color-text);
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .activity-agent {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .activity-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .view-all-btn {
    align-self: flex-start;
    background: transparent;
    border: none;
    color: var(--color-primary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    padding: 0;
    text-decoration: underline;
    text-underline-offset: 2px;
    transition: opacity var(--transition-fast);
  }

  .view-all-btn:hover { opacity: 0.75; }

  /* Quick links */
  .quick-links {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-3);
  }

  .quick-link {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: border-color var(--transition-fast), color var(--transition-fast), background var(--transition-fast);
  }

  .quick-link:hover {
    border-color: var(--color-primary);
    color: var(--color-text);
    background: color-mix(in srgb, var(--color-primary) 4%, var(--color-surface));
  }

  .quick-link-icon {
    font-size: 1rem;
  }
</style>
