<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let entries = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let cancellingId = $state(null);

  $effect(() => {
    load();
  });

  async function load() {
    loading = true;
    error = null;
    try {
      entries = await api.mergeQueue();
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function cancel(id) {
    cancellingId = id;
    try {
      await api.cancelQueueEntry(id);
      entries = entries.filter((e) => e.id !== id);
      toastSuccess('Queue entry cancelled.');
    } catch (e) {
      toastError(e.message);
    } finally {
      cancellingId = null;
    }
  }

  function relativeTime(ts) {
    const diff = Math.floor((Date.now() - ts * 1000) / 1000);
    if (diff < 60) return `${diff}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  // Group entries into lanes
  let queued     = $derived(entries.filter(e => e.status === 'queued'));
  let processing = $derived(entries.filter(e => e.status === 'processing'));
  let done       = $derived(entries.filter(e => e.status === 'merged' || e.status === 'failed' || e.status === 'cancelled'));
</script>

<div class="panel">
  <div class="panel-header">
    <div class="header-left">
      <h2>Merge Queue</h2>
      <span class="queue-count">{entries.length} entries</span>
    </div>
    <button class="refresh-btn" onclick={load} disabled={loading}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
        <path d="M23 4v6h-6M1 20v-6h6"/><path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15"/>
      </svg>
      {loading ? 'Loading…' : 'Refresh'}
    </button>
  </div>

  <div class="scroll">
    {#if loading}
      <div class="skeleton-panel">
        <Skeleton height="120px" />
        <Skeleton height="120px" />
        <Skeleton height="120px" />
      </div>
    {:else if error}
      <div class="error-msg">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16"><circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/></svg>
        {error}
      </div>
    {:else if entries.length === 0}
      <EmptyState
        title="Queue is empty"
        description="Approved merge requests will appear here when added to the merge queue."
      />
    {:else}
      <!-- Visual flow lanes -->
      <div class="flow-lanes">
        <!-- Queued lane -->
        <div class="lane">
          <div class="lane-header">
            <div class="lane-indicator queued-indicator"></div>
            <span class="lane-title">Queued</span>
            <span class="lane-count">{queued.length}</span>
          </div>
          <div class="lane-cards">
            {#if queued.length === 0}
              <div class="lane-empty">No entries queued</div>
            {:else}
              {#each queued as entry (entry.id)}
                <div class="queue-card queued">
                  <div class="card-top">
                    <code class="mr-id">{entry.merge_request_id.slice(0, 10)}…</code>
                    <Badge value={entry.priority} />
                  </div>
                  <div class="card-meta">
                    <span class="enqueued-time">Enqueued {relativeTime(entry.enqueued_at)}</span>
                  </div>
                  <div class="card-actions">
                    <button
                      class="cancel-btn"
                      onclick={() => cancel(entry.id)}
                      disabled={cancellingId === entry.id}
                    >
                      {cancellingId === entry.id ? 'Cancelling…' : 'Cancel'}
                    </button>
                  </div>
                </div>
              {/each}
            {/if}
          </div>
        </div>

        <!-- Arrow -->
        <div class="flow-arrow">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="20" height="20">
            <path d="M5 12h14M12 5l7 7-7 7"/>
          </svg>
        </div>

        <!-- Processing lane -->
        <div class="lane">
          <div class="lane-header">
            <div class="lane-indicator processing-indicator pulse"></div>
            <span class="lane-title">Processing</span>
            <span class="lane-count">{processing.length}</span>
          </div>
          <div class="lane-cards">
            {#if processing.length === 0}
              <div class="lane-empty">Nothing processing</div>
            {:else}
              {#each processing as entry (entry.id)}
                <div class="queue-card processing">
                  <div class="card-top">
                    <code class="mr-id">{entry.merge_request_id.slice(0, 10)}…</code>
                    <Badge value={entry.priority} />
                  </div>
                  <div class="card-meta">
                    <span class="enqueued-time">Started {relativeTime(entry.enqueued_at)}</span>
                  </div>
                  <div class="card-actions">
                    <button
                      class="cancel-btn"
                      onclick={() => cancel(entry.id)}
                      disabled={cancellingId === entry.id}
                    >
                      {cancellingId === entry.id ? 'Cancelling…' : 'Cancel'}
                    </button>
                  </div>
                </div>
              {/each}
            {/if}
          </div>
        </div>

        <!-- Arrow -->
        <div class="flow-arrow">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="20" height="20">
            <path d="M5 12h14M12 5l7 7-7 7"/>
          </svg>
        </div>

        <!-- Done lane -->
        <div class="lane">
          <div class="lane-header">
            <div class="lane-indicator done-indicator"></div>
            <span class="lane-title">Done</span>
            <span class="lane-count">{done.length}</span>
          </div>
          <div class="lane-cards">
            {#if done.length === 0}
              <div class="lane-empty">No completed entries</div>
            {:else}
              {#each done.slice(0, 5) as entry (entry.id)}
                <div class="queue-card done" class:failed={entry.status === 'failed'}>
                  <div class="card-top">
                    <code class="mr-id">{entry.merge_request_id.slice(0, 10)}…</code>
                    <Badge value={entry.status} />
                  </div>
                  <div class="card-meta">
                    <span class="enqueued-time">
                      {entry.processed_at ? relativeTime(entry.processed_at) : relativeTime(entry.enqueued_at)}
                    </span>
                    {#if entry.error_message}
                      <span class="error-hint" title={entry.error_message}>⚠ error</span>
                    {/if}
                  </div>
                </div>
              {/each}
            {/if}
          </div>
        </div>
      </div>

      <!-- All entries table -->
      {#if entries.length > 3}
        <div class="all-entries">
          <h3 class="all-entries-title">All Entries</h3>
          <table class="entries-table">
            <thead>
              <tr>
                <th>MR ID</th>
                <th>Priority</th>
                <th>Status</th>
                <th>Enqueued</th>
                <th>Processed</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {#each entries as entry (entry.id)}
                <tr>
                  <td><code class="mr-id-sm">{entry.merge_request_id.slice(0, 12)}…</code></td>
                  <td><Badge value={entry.priority} /></td>
                  <td><Badge value={entry.status} /></td>
                  <td class="dim">{relativeTime(entry.enqueued_at)}</td>
                  <td class="dim">{entry.processed_at ? relativeTime(entry.processed_at) : '—'}</td>
                  <td>
                    {#if entry.status === 'queued' || entry.status === 'processing'}
                      <button
                        class="cancel-btn-sm"
                        onclick={() => cancel(entry.id)}
                        disabled={cancellingId === entry.id}
                      >
                        {cancellingId === entry.id ? '…' : 'Cancel'}
                      </button>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  h2 {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .queue-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
  }

  .refresh-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    transition: border-color var(--transition-fast), color var(--transition-fast);
    font-family: var(--font-body);
  }
  .refresh-btn:hover:not(:disabled) {
    border-color: var(--color-border-strong);
    color: var(--color-text);
  }
  .refresh-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .skeleton-panel {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .error-msg {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-danger);
    font-size: var(--text-sm);
    padding: var(--space-4);
  }

  /* Flow lanes */
  .flow-lanes {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .lane {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
  }

  .lane-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }

  .lane-indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .queued-indicator     { background: var(--color-queue-queued); }
  .processing-indicator { background: var(--color-queue-processing); }
  .done-indicator       { background: var(--color-success); }

  @keyframes pulse-dot {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0.4; }
  }

  .pulse { animation: pulse-dot 1.5s ease-in-out infinite; }

  .lane-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    flex: 1;
  }

  .lane-count {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
  }

  .lane-cards {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .lane-empty {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    text-align: center;
    padding: var(--space-4);
    background: var(--color-surface);
    border: 1px dashed var(--color-border);
    border-radius: var(--radius);
  }

  /* Queue cards */
  .queue-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    transition: border-color var(--transition-fast);
  }

  .queue-card:hover { border-color: var(--color-border-strong); }

  .queue-card.processing {
    border-color: rgba(245, 146, 27, 0.4);
    animation: processing-pulse 2s ease-in-out infinite;
  }

  @keyframes processing-pulse {
    0%, 100% { border-color: rgba(245, 146, 27, 0.4); }
    50%       { border-color: rgba(245, 146, 27, 0.8); }
  }

  .queue-card.failed { border-color: rgba(240, 86, 29, 0.4); }

  .card-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .mr-id {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .card-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2);
  }

  .enqueued-time {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .error-hint {
    font-size: var(--text-xs);
    color: var(--color-danger);
  }

  .card-actions { display: flex; gap: var(--space-2); }

  .cancel-btn {
    background: rgba(240, 86, 29, 0.1);
    border: 1px solid rgba(240, 86, 29, 0.3);
    border-radius: var(--radius);
    color: var(--color-danger);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    font-family: var(--font-body);
    transition: background var(--transition-fast);
    width: 100%;
  }
  .cancel-btn:hover:not(:disabled) { background: rgba(240, 86, 29, 0.2); }
  .cancel-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  /* Flow arrows */
  .flow-arrow {
    flex-shrink: 0;
    color: var(--color-text-muted);
    margin-top: 2.5rem;
  }

  /* All entries table */
  .all-entries {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .all-entries-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .entries-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .entries-table thead th {
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .entries-table tbody tr {
    border-bottom: 1px solid var(--color-border);
    transition: background var(--transition-fast);
  }
  .entries-table tbody tr:last-child { border-bottom: none; }
  .entries-table tbody tr:hover { background: var(--color-surface-elevated); }

  .entries-table td {
    padding: var(--space-3) var(--space-4);
    vertical-align: middle;
  }

  .mr-id-sm {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .dim { color: var(--color-text-muted); font-size: var(--text-xs); }

  .cancel-btn-sm {
    background: rgba(240, 86, 29, 0.1);
    border: 1px solid rgba(240, 86, 29, 0.3);
    border-radius: var(--radius-sm);
    color: var(--color-danger);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 2px var(--space-2);
    font-family: var(--font-body);
  }
  .cancel-btn-sm:disabled { opacity: 0.4; cursor: not-allowed; }
</style>
