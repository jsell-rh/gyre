<script>
  /**
   * DependencyDetail — panel shown when a dependency edge is clicked in the
   * cross-repo dependency graph (dependency-graph.md §UI dependency detail).
   *
   * Shows: source/target repos and artifacts, version info (pinned/current),
   * version drift, breaking changes with acknowledgment status, and an
   * acknowledge action for unacknowledged breaking changes.
   *
   * Props:
   *   edge          — graph edge (GraphEdgeResponse): source, target, type,
   *                   status, source_artifact, target_artifact,
   *                   version_pinned, target_version_current, version_drift,
   *                   breaking_changes[]
   *   nodes         — GraphNode[] for repo id → name resolution
   *   onClose       — () => void
   *   onAcknowledge — (bcId: string) => Promise<void> | void — called after a
   *                   breaking change is acknowledged so the parent can reload
   *                   the graph (edge status may revert to active).
   */
  import { api } from '../lib/api.js';

  let {
    edge = null,
    nodes = [],
    onClose = () => {},
    onAcknowledge = () => {},
  } = $props();

  let ackError = $state(null);
  let ackPendingId = $state(null);

  const nameById = $derived.by(() => {
    const map = new Map();
    for (const n of nodes ?? []) map.set(n.repo_id, n.name ?? n.repo_id);
    return map;
  });

  function repoName(id) {
    return nameById.get(id) ?? id;
  }

  function fmt(v, fallback = '—') {
    return v == null || v === '' ? fallback : v;
  }

  function statusVariant(status) {
    switch ((status ?? '').toLowerCase()) {
      case 'active': return 'ok';
      case 'stale': return 'stale';
      case 'breaking': return 'breaking';
      case 'orphaned': return 'orphaned';
      default: return 'orphaned';
    }
  }

  async function acknowledge(bc) {
    if (ackPendingId) return;
    ackPendingId = bc.id;
    ackError = null;
    try {
      await api.acknowledgeBreakingChange(bc.id);
      await onAcknowledge(bc.id);
    } catch (e) {
      ackError = e?.message ?? 'Failed to acknowledge breaking change';
    } finally {
      ackPendingId = null;
    }
  }
</script>

{#if edge}
  <aside class="dep-detail" data-testid="dep-detail" aria-label="Dependency detail">
    <div class="dep-detail-header">
      <h3 class="dep-detail-title" data-testid="dep-detail-title">
        {repoName(edge.source)} → {repoName(edge.target)}
      </h3>
      <button
        class="dep-detail-close"
        onclick={onClose}
        aria-label="Close dependency detail"
        type="button"
      >✕</button>
    </div>

    <div class="dep-detail-row">
      <span class="dep-detail-label">Type</span>
      <span class="dep-detail-value" data-testid="dep-detail-type">{fmt(edge.type)}</span>
    </div>
    <div class="dep-detail-row">
      <span class="dep-detail-label">Status</span>
      <span class="dep-detail-value dep-status dep-status-{statusVariant(edge.status)}" data-testid="dep-detail-status">
        {fmt(edge.status)}
      </span>
    </div>
    <div class="dep-detail-row">
      <span class="dep-detail-label">Source artifact</span>
      <span class="dep-detail-value dep-mono" data-testid="dep-detail-source-artifact">{fmt(edge.source_artifact)}</span>
    </div>
    <div class="dep-detail-row">
      <span class="dep-detail-label">Target artifact</span>
      <span class="dep-detail-value dep-mono" data-testid="dep-detail-target-artifact">{fmt(edge.target_artifact)}</span>
    </div>
    <div class="dep-detail-row">
      <span class="dep-detail-label">Version pinned</span>
      <span class="dep-detail-value dep-mono" data-testid="dep-detail-pinned">{fmt(edge.version_pinned)}</span>
    </div>
    <div class="dep-detail-row">
      <span class="dep-detail-label">Version current</span>
      <span class="dep-detail-value dep-mono" data-testid="dep-detail-current">{fmt(edge.target_version_current)}</span>
    </div>
    <div class="dep-detail-row">
      <span class="dep-detail-label">Version drift</span>
      <span class="dep-detail-value dep-mono" data-testid="dep-detail-drift">
        {edge.version_drift == null ? '—' : `${edge.version_drift} version${edge.version_drift === 1 ? '' : 's'}`}
      </span>
    </div>

    <div class="dep-detail-section">
      <h4 class="dep-detail-section-title">Breaking changes</h4>
      {#if ackError}
        <p class="dep-detail-error" role="alert" data-testid="dep-detail-ack-error">{ackError}</p>
      {/if}
      {#if !edge.breaking_changes || edge.breaking_changes.length === 0}
        <p class="dep-detail-muted" data-testid="dep-detail-no-breaking">No breaking changes recorded.</p>
      {:else}
        <ul class="dep-bc-list" data-testid="dep-detail-breaking-list">
          {#each edge.breaking_changes as bc (bc.id)}
            <li class="dep-bc-item" data-testid="dep-detail-bc-{bc.id}">
              <div class="dep-bc-head">
                <span class="dep-bc-sha dep-mono">{bc.commit_sha?.slice(0, 10) || '—'}</span>
                <span
                  class="dep-bc-ack dep-status dep-status-{bc.acknowledged ? 'ok' : 'breaking'}"
                  data-testid="dep-detail-bc-status-{bc.id}"
                >
                  {bc.acknowledged ? 'Acknowledged' : 'Unacknowledged'}
                </span>
              </div>
              {#if bc.description}
                <p class="dep-bc-desc">{bc.description}</p>
              {/if}
              {#if !bc.acknowledged}
                <button
                  class="dep-bc-ack-btn"
                  type="button"
                  disabled={ackPendingId === bc.id}
                  onclick={() => acknowledge(bc)}
                  data-testid="dep-detail-ack-{bc.id}"
                >
                  {ackPendingId === bc.id ? 'Acknowledging…' : 'Acknowledge'}
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </aside>
{/if}

<style>
  .dep-detail {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px;
    background: var(--color-surface-raised, #1e293b);
    border: 1px solid var(--color-border, #2a2a3a);
    border-radius: var(--radius, 6px);
    font-size: var(--text-sm, 0.875rem);
    color: var(--color-text, #e5e7eb);
    min-width: 260px;
    max-width: 340px;
    overflow-y: auto;
  }

  .dep-detail-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .dep-detail-title {
    margin: 0;
    font-size: var(--text-sm, 0.875rem);
    font-weight: 600;
    color: var(--color-text, #e5e7eb);
  }

  .dep-detail-close {
    background: transparent;
    border: none;
    color: var(--color-text-muted, #888);
    cursor: pointer;
    font-size: 1rem;
    padding: 2px 6px;
  }

  .dep-detail-close:hover {
    color: var(--color-text, #e5e7eb);
  }

  .dep-detail-row {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }

  .dep-detail-label {
    color: var(--color-text-muted, #888);
    white-space: nowrap;
  }

  .dep-detail-value {
    text-align: right;
    word-break: break-word;
  }

  .dep-mono {
    font-family: var(--font-mono, monospace);
  }

  .dep-detail-section {
    border-top: 1px solid var(--color-border, #2a2a3a);
    padding-top: 8px;
    margin-top: 4px;
  }

  .dep-detail-section-title {
    margin: 0 0 6px;
    font-size: var(--text-xs, 0.75rem);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted, #888);
  }

  .dep-detail-muted {
    margin: 0;
    color: var(--color-text-muted, #888);
    font-size: var(--text-xs, 0.75rem);
  }

  .dep-detail-error {
    margin: 0 0 6px;
    color: #f87171;
    font-size: var(--text-xs, 0.75rem);
  }

  .dep-bc-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .dep-bc-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 8px;
    background: var(--color-surface, #0a0a0f);
    border: 1px solid var(--color-border, #2a2a3a);
    border-radius: var(--radius, 6px);
  }

  .dep-bc-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .dep-bc-sha {
    color: var(--color-text-muted, #888);
    font-size: var(--text-xs, 0.75rem);
  }

  .dep-bc-desc {
    margin: 0;
    font-size: var(--text-xs, 0.75rem);
    color: var(--color-text, #e5e7eb);
  }

  .dep-bc-ack-btn {
    align-self: flex-start;
    padding: 3px 10px;
    font-size: var(--text-xs, 0.75rem);
    font-family: var(--font-body, system-ui);
    color: var(--color-text, #e5e7eb);
    background: var(--color-surface-raised, #1e293b);
    border: 1px solid #ef4444;
    border-radius: var(--radius, 6px);
    cursor: pointer;
  }

  .dep-bc-ack-btn:hover:not(:disabled) {
    border-color: #f87171;
  }

  .dep-bc-ack-btn:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .dep-status {
    font-size: var(--text-xs, 0.75rem);
    padding: 1px 8px;
    border-radius: 999px;
    white-space: nowrap;
  }

  .dep-status-ok {
    color: #22c55e;
    background: rgba(34, 197, 94, 0.12);
  }

  .dep-status-stale {
    color: #eab308;
    background: rgba(234, 179, 8, 0.12);
  }

  .dep-status-breaking {
    color: #ef4444;
    background: rgba(239, 68, 68, 0.12);
  }

  .dep-status-orphaned {
    color: #9ca3af;
    background: rgba(156, 163, 175, 0.12);
  }
</style>
