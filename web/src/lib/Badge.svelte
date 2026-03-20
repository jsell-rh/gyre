<script>
  let { value, variant = 'default' } = $props();

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
    value?.replace?.(/([A-Z])/g, ' $1').trim() ?? value
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

  .badge-success  { background: rgba(99,153,61,0.15);  color: #7dc25a; border-color: rgba(99,153,61,0.3); }
  .badge-warning  { background: rgba(245,146,27,0.15); color: #f5921b; border-color: rgba(245,146,27,0.3); }
  .badge-danger   { background: rgba(240,86,29,0.15);  color: #f0561d; border-color: rgba(240,86,29,0.3); }
  .badge-info     { background: rgba(0,102,204,0.15);  color: #4394e5; border-color: rgba(0,102,204,0.3); }
  .badge-purple   { background: rgba(94,64,190,0.15);  color: #8b6fe0; border-color: rgba(94,64,190,0.3); }
  .badge-blocked  { background: rgba(94,64,190,0.15);  color: #8b6fe0; border-color: rgba(94,64,190,0.3); }
  .badge-muted    { background: rgba(112,112,112,0.15); color: #a3a3a3; border-color: rgba(112,112,112,0.3); }
  .badge-critical { background: rgba(238,0,0,0.15);    color: #ee0000; border-color: rgba(238,0,0,0.3); }
  .badge-default  { background: rgba(112,112,112,0.15); color: #a3a3a3; border-color: rgba(112,112,112,0.3); }
</style>
