<script>
  let { value, type = 'status' } = $props();

  const statusColors = {
    Idle: 'badge-idle',
    Active: 'badge-active',
    Blocked: 'badge-blocked',
    Error: 'badge-error',
    Dead: 'badge-dead',
    Backlog: 'badge-idle',
    InProgress: 'badge-active',
    Review: 'badge-review',
    Done: 'badge-done',
    Blocked: 'badge-blocked',
  };

  const priorityColors = {
    Low: 'badge-low',
    Medium: 'badge-medium',
    High: 'badge-high',
    Critical: 'badge-critical',
  };

  let cls = $derived(
    type === 'priority' ? (priorityColors[value] ?? 'badge-idle') : (statusColors[value] ?? 'badge-idle')
  );
</script>

<span class="badge {cls}">{value}</span>

<style>
  .badge {
    display: inline-block;
    padding: 0.15rem 0.45rem;
    border-radius: 3px;
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    white-space: nowrap;
  }
  .badge-active  { background: #22c55e22; color: #4ade80; border: 1px solid #22c55e44; }
  .badge-idle    { background: #64748b22; color: #94a3b8; border: 1px solid #64748b44; }
  .badge-blocked { background: #f9731622; color: #fb923c; border: 1px solid #f9731644; }
  .badge-error   { background: #ef444422; color: #f87171; border: 1px solid #ef444444; }
  .badge-dead    { background: #1e293b;   color: #475569; border: 1px solid #334155; }
  .badge-review  { background: #a855f722; color: #c084fc; border: 1px solid #a855f744; }
  .badge-done    { background: #3b82f622; color: #60a5fa; border: 1px solid #3b82f644; }
  .badge-low     { background: #64748b22; color: #94a3b8; border: 1px solid #64748b44; }
  .badge-medium  { background: #eab30822; color: #fbbf24; border: 1px solid #eab30844; }
  .badge-high    { background: #f9731622; color: #fb923c; border: 1px solid #f9731644; }
  .badge-critical{ background: #ef444422; color: #f87171; border: 1px solid #ef444444; }
</style>
