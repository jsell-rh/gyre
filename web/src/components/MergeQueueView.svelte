<script>
  import { api } from '../lib/api.js';

  let entries = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let cancellingId = $state(null);

  $effect(() => {
    load();
  });

  async function load() {
    loading = true; error = null;
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
    } catch (e) {
      error = e.message;
    } finally {
      cancellingId = null;
    }
  }

  function formatDate(ts) {
    return new Date(ts * 1000).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });
  }

  const statusStyles = {
    queued:     { color: '#60a5fa', bg: '#60a5fa18' },
    processing: { color: '#facc15', bg: '#facc1518' },
    merged:     { color: '#4ade80', bg: '#4ade8018' },
    failed:     { color: '#f87171', bg: '#f8717118' },
    cancelled:  { color: '#94a3b8', bg: '#94a3b818' },
  };
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Merge Queue</h2>
    <button class="refresh-btn" onclick={load}>↺ Refresh</button>
  </div>

  {#if loading}
    <p class="state-msg">Loading…</p>
  {:else if error}
    <p class="state-msg error">{error}</p>
  {:else if entries.length === 0}
    <p class="state-msg muted">Queue is empty.</p>
  {:else}
    <div class="scroll">
      <table class="queue-table">
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
            {@const style = statusStyles[entry.status] ?? statusStyles.cancelled}
            <tr>
              <td>
                <code class="mr-id">{entry.merge_request_id.slice(0, 12)}…</code>
              </td>
              <td class="priority">{entry.priority}</td>
              <td>
                <span class="status-badge" style:color={style.color} style:background={style.bg}>
                  {entry.status}
                </span>
              </td>
              <td class="date">{formatDate(entry.enqueued_at)}</td>
              <td class="date">{entry.processed_at ? formatDate(entry.processed_at) : '—'}</td>
              <td>
                {#if entry.status === 'queued' || entry.status === 'processing'}
                  <button
                    class="cancel-btn"
                    onclick={() => cancel(entry.id)}
                    disabled={cancellingId === entry.id}
                  >
                    {cancellingId === entry.id ? '…' : 'Cancel'}
                  </button>
                {/if}
                {#if entry.error_message}
                  <span class="error-msg" title={entry.error_message}>⚠</span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 1rem 1.25rem; border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  h2 { margin: 0; font-size: 1rem; font-weight: 600; color: var(--text); }

  .refresh-btn {
    background: none; border: 1px solid var(--border); border-radius: 4px;
    color: var(--text-muted); cursor: pointer; font-size: 0.82rem;
    padding: 0.3rem 0.65rem; transition: color 0.1s, border-color 0.1s;
  }
  .refresh-btn:hover { color: var(--text); border-color: var(--accent); }

  .scroll { flex: 1; overflow: auto; padding: 0.75rem 1.25rem; }

  .queue-table {
    width: 100%; border-collapse: collapse; font-size: 0.85rem;
  }

  thead th {
    text-align: left; font-size: 0.75rem; font-weight: 600; color: var(--text-dim);
    text-transform: uppercase; letter-spacing: 0.04em;
    padding: 0.4rem 0.75rem; border-bottom: 1px solid var(--border);
  }

  tbody tr { border-bottom: 1px solid var(--border-subtle); }
  tbody tr:last-child { border-bottom: none; }
  tbody tr:hover { background: var(--surface-hover); }

  td { padding: 0.55rem 0.75rem; color: var(--text); vertical-align: middle; }

  .mr-id { font-family: 'Courier New', monospace; font-size: 0.78rem; color: var(--text-muted); }
  .priority { color: var(--text-muted); text-align: center; }
  .date { font-size: 0.78rem; color: var(--text-dim); white-space: nowrap; }

  .status-badge {
    display: inline-block; font-size: 0.72rem; font-weight: 600;
    text-transform: uppercase; letter-spacing: 0.04em;
    padding: 0.15rem 0.5rem; border-radius: 3px;
  }

  .cancel-btn {
    background: none; border: 1px solid #f8717166; border-radius: 4px;
    color: #f87171; cursor: pointer; font-size: 0.78rem;
    padding: 0.2rem 0.5rem; transition: background 0.1s;
  }
  .cancel-btn:hover:not(:disabled) { background: #f8717118; }
  .cancel-btn:disabled { opacity: 0.4; cursor: not-allowed; }

  .error-msg { color: #f87171; cursor: help; margin-left: 0.4rem; }

  .state-msg { padding: 2rem; color: var(--text-dim); text-align: center; }
  .state-msg.error { color: #f87171; }
  .state-msg.muted { font-style: italic; }
</style>
