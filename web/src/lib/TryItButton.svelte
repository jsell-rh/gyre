<script>
  import { api } from './api.js';

  /**
   * "Try it" action for merged work (activity-dashboard.md §"Try it" links):
   * provisions an ephemeral preview environment running the MR's landed
   * commit, shows its live status, and tears it down. Mounted from merged-MR
   * activity entries and the MR detail surface.
   */
  let { mrId, compact = false } = $props();

  let loading = $state(false);
  let error = $state(null);
  let environment = $state(null);
  let pollTimer = null;

  const STATUS_LABELS = {
    provisioning: 'Starting preview…',
    active: 'Preview running',
    expired: 'Preview ended',
    failed: 'Preview failed',
  };

  async function loadExisting() {
    try {
      const envs = await api.mrTryItEnvironments(mrId);
      const list = Array.isArray(envs) ? envs : [];
      // Newest first; show the latest environment (live one if present).
      environment = list.find((e) => e.status === 'active' || e.status === 'provisioning') ?? list[0] ?? null;
      schedulePoll();
    } catch {
      environment = null;
    }
  }

  function schedulePoll() {
    clearTimeout(pollTimer);
    if (environment && (environment.status === 'provisioning' || environment.status === 'active')) {
      pollTimer = setTimeout(poll, 2000);
    }
  }

  async function poll() {
    if (!environment) return;
    try {
      const env = await api.previewEnvironment(environment.id);
      environment = env ?? environment;
    } catch {
      /* transient poll failure — keep the last known status */
    }
    schedulePoll();
  }

  async function provision() {
    loading = true;
    error = null;
    try {
      const env = await api.tryIt(mrId);
      environment = env ?? null;
      schedulePoll();
    } catch (e) {
      error = e?.message ?? 'provisioning failed';
    } finally {
      loading = false;
    }
  }

  async function teardown() {
    if (!environment) return;
    loading = true;
    try {
      await api.teardownPreviewEnvironment(environment.id);
      environment = { ...environment, status: 'expired', end_reason: 'torn-down' };
    } catch (e) {
      error = e?.message ?? 'teardown failed';
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (mrId) void loadExisting();
    return () => clearTimeout(pollTimer);
  });
</script>

<div class="try-it {compact ? 'try-it-compact' : ''}">
  {#if environment && (environment.status === 'active' || environment.status === 'provisioning')}
    <span class="try-it-status try-it-status-{environment.status}" title={`commit ${environment.commit_sha ?? ''}`}>
      {STATUS_LABELS[environment.status] ?? environment.status}
    </span>
    {#if environment.status === 'active' && environment.preview_url}
      <a class="try-it-link" href={environment.preview_url} target="_blank" rel="noopener noreferrer">Open</a>
    {/if}
    <button class="try-it-btn try-it-teardown" onclick={teardown} disabled={loading}>
      Tear down
    </button>
  {:else if environment && environment.status === 'failed'}
    <span class="try-it-status try-it-status-failed" title={environment.end_reason ?? ''}>
      {STATUS_LABELS[environment.status] ?? environment.status}
    </span>
    <button class="try-it-btn" onclick={provision} disabled={loading}>Try again</button>
  {:else}
    <button class="try-it-btn" onclick={provision} disabled={loading}>
      {loading ? 'Starting…' : error ? 'Try again' : 'Try it'}
    </button>
  {/if}
  {#if error}
    <span class="try-it-error">{error}</span>
  {/if}
</div>

<style>
  .try-it {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2, 6px);
    font-size: 11px;
  }
  .try-it-compact {
    font-size: 10px;
  }
  .try-it-btn {
    font: inherit;
    padding: 2px 8px;
    border-radius: var(--radius-sm, 4px);
    border: 1px solid var(--color-border, #ccc);
    background: var(--color-surface, #fafafa);
    cursor: pointer;
  }
  .try-it-btn:hover:not(:disabled) {
    background: var(--color-surface-elevated, #f0f0f0);
  }
  .try-it-btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .try-it-teardown {
    color: var(--color-danger, #b00);
  }
  .try-it-status {
    color: var(--color-text-muted, #666);
  }
  .try-it-status-active {
    color: var(--color-success, #080);
  }
  .try-it-status-failed {
    color: var(--color-danger, #b00);
  }
  .try-it-link {
    color: var(--color-link, #06c);
  }
  .try-it-error {
    color: var(--color-danger, #b00);
  }
</style>
