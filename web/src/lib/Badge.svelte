<script>
  let { value = '', variant = 'default', role: roleAttr = 'status', 'aria-label': ariaLabel = undefined } = $props();

  /* Map common status strings to badge variants */
  const STATUS_MAP = {
    // Agent (per agent-runtime.md §1: Active, Idle, Failed, Stopped, Dead)
    Active: 'success', active: 'success',
    Idle: 'muted', idle: 'muted',
    Stopped: 'warning', stopped: 'warning',
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

<span class="badge badge-{resolvedVariant}" role={roleAttr} aria-label={ariaLabel}>{label}</span>

<style>
  .badge {
    display: inline-block;
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    white-space: nowrap;
    border: 1px solid transparent;
  }

  .badge-success  { background: color-mix(in srgb, var(--color-success) 15%, transparent);  color: var(--color-success); border-color: color-mix(in srgb, var(--color-success) 30%, transparent); }
  .badge-warning  { background: color-mix(in srgb, var(--color-warning) 15%, transparent); color: var(--color-warning); border-color: color-mix(in srgb, var(--color-warning) 30%, transparent); }
  .badge-danger   { background: color-mix(in srgb, var(--color-danger) 15%, transparent);  color: var(--color-danger); border-color: color-mix(in srgb, var(--color-danger) 30%, transparent); }
  .badge-info     { background: color-mix(in srgb, var(--color-info) 15%, transparent);  color: var(--color-info); border-color: color-mix(in srgb, var(--color-info) 30%, transparent); }
  .badge-purple   { background: color-mix(in srgb, var(--color-purple, #8b6fe0) 15%, transparent);  color: var(--color-purple, #8b6fe0); border-color: color-mix(in srgb, var(--color-purple, #8b6fe0) 30%, transparent); }
  .badge-blocked  { background: color-mix(in srgb, var(--color-blocked) 15%, transparent);  color: var(--color-blocked); border-color: color-mix(in srgb, var(--color-blocked) 30%, transparent); }
  .badge-muted    { background: color-mix(in srgb, var(--color-text-muted) 15%, transparent); color: var(--color-text-muted); border-color: color-mix(in srgb, var(--color-text-muted) 30%, transparent); }
  .badge-critical { background: color-mix(in srgb, var(--color-danger) 15%, transparent);    color: var(--color-danger); border-color: color-mix(in srgb, var(--color-danger) 30%, transparent); }
  .badge-default  { background: color-mix(in srgb, var(--color-text-muted) 15%, transparent); color: var(--color-text-muted); border-color: color-mix(in srgb, var(--color-text-muted) 30%, transparent); }
</style>
