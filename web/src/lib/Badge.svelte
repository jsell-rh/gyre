<script>
  /**
   * Badge — compatibility facade over the shadcn-svelte Badge
   * (lib/components/ui/badge/). Consumed contract unchanged: `value`
   * (status string, mapped to a variant + humanized label), `variant`
   * override, `role` (default status), `aria-label`, `title`.
   * Styling comes from cva variants + Tailwind; the bespoke
   * `.badge .badge-{variant}` class layer is retired.
   */
  import { Badge } from './components/ui/badge/index.js';

  let { value = '', variant = 'default', role: roleAttr = 'status', 'aria-label': ariaLabel = undefined, title = undefined } = $props();

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

<Badge variant={resolvedVariant} class="badge badge-{resolvedVariant}" role={roleAttr} aria-label={ariaLabel} {title}>{label}</Badge>
