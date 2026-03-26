<script>
  let { value = '', variant = 'default' } = $props();

  /* Map common status strings to badge variants */
  const STATUS_MAP = {
    // Agent
    Active: 'success', active: 'success',
    Idle: 'muted', idle: 'muted',
    Blocked: 'blocked', blocked: 'blocked',
    Error: 'danger', error: 'danger',
    Dead: 'muted', dead: 'muted',
    // Task
    Backlog: 'muted', backlog: 'muted',
    InProgress: 'warning', in_progress: 'warning',
    Review: 'info', review: 'info',
    Done: 'success', done: 'success',
    // MR
    Open: 'info', open: 'info',
    Approved: 'success', approved: 'success',
    Merged: 'purple', merged: 'purple',
    Closed: 'muted', closed: 'muted',
    // Queue
    Queued: 'info', queued: 'info',
    Processing: 'warning', processing: 'warning',
    Failed: 'danger', failed: 'danger',
    // Priority
    Low: 'muted', low: 'muted',
    Medium: 'warning', medium: 'warning',
    High: 'danger', high: 'danger',
    Critical: 'critical', critical: 'critical',
  };

  let resolvedVariant = $derived(
    variant !== 'default' ? variant : (STATUS_MAP[value] ?? 'muted')
  );

  let label = $derived(
    value?.replace?.(/([A-Z])/g, ' $1').trim() ?? value ?? ''
  );
</script>

<span class="badge badge-{resolvedVariant}">{label}</span>

<style>
  .badge {
    display: inline-block;
    padding: 0.15rem 0.45rem;
    border-radius: var(--radius-sm);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    white-space: nowrap;
    border: 1px solid transparent;
  }

  .badge-success  { background: color-mix(in srgb, var(--color-success, #22c55e) 15%, transparent);  color: var(--color-success, #22c55e); border-color: color-mix(in srgb, var(--color-success, #22c55e) 30%, transparent); }
  .badge-warning  { background: color-mix(in srgb, var(--color-warning, #f59e0b) 15%, transparent); color: var(--color-warning, #f59e0b); border-color: color-mix(in srgb, var(--color-warning, #f59e0b) 30%, transparent); }
  .badge-danger   { background: color-mix(in srgb, var(--color-danger, #f0561d) 15%, transparent);  color: var(--color-danger, #f0561d); border-color: color-mix(in srgb, var(--color-danger, #f0561d) 30%, transparent); }
  .badge-info     { background: color-mix(in srgb, var(--color-info, #60a5fa) 15%, transparent);  color: var(--color-info, #60a5fa); border-color: color-mix(in srgb, var(--color-info, #60a5fa) 30%, transparent); }
  .badge-purple   { background: color-mix(in srgb, var(--color-purple, #8b6fe0) 15%, transparent);  color: var(--color-purple, #8b6fe0); border-color: color-mix(in srgb, var(--color-purple, #8b6fe0) 30%, transparent); }
  .badge-blocked  { background: color-mix(in srgb, var(--color-purple, #8b6fe0) 15%, transparent);  color: var(--color-purple, #8b6fe0); border-color: color-mix(in srgb, var(--color-purple, #8b6fe0) 30%, transparent); }
  .badge-muted    { background: color-mix(in srgb, var(--color-text-muted, #a3a3a3) 15%, transparent); color: var(--color-text-muted, #a3a3a3); border-color: color-mix(in srgb, var(--color-text-muted, #a3a3a3) 30%, transparent); }
  .badge-critical { background: color-mix(in srgb, var(--color-danger, #ee0000) 15%, transparent);    color: var(--color-danger, #ee0000); border-color: color-mix(in srgb, var(--color-danger, #ee0000) 30%, transparent); }
  .badge-default  { background: color-mix(in srgb, var(--color-text-muted, #a3a3a3) 15%, transparent); color: var(--color-text-muted, #a3a3a3); border-color: color-mix(in srgb, var(--color-text-muted, #a3a3a3) 30%, transparent); }
</style>
