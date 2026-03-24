<script>
  import { onMount, onDestroy } from 'svelte';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Button from '../lib/Button.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Skeleton from '../lib/Skeleton.svelte';

  const SEEN_KEY = 'gyre_inbox_seen';

  let items = $state([]);
  let loading = $state(true);
  let error = $state(null);
  let seenIds = $state(new Set(JSON.parse(localStorage.getItem(SEEN_KEY) || '[]')));
  // Map of item.id -> { loading, success, message }
  let actionStates = $state({});

  let refreshInterval;

  let pendingCount = $derived(items.filter(i => !seenIds.has(i.id)).length);

  async function loadInbox() {
    try {
      const [mrs, pendingSpecs] = await Promise.allSettled([
        api.mergeRequests({ status: 'review' }),
        api.getPendingSpecs(),
      ]);

      const inbox = [];

      const mrList = mrs.status === 'fulfilled' ? (mrs.value || []) : [];

      // MR review items
      for (const mr of mrList) {
        inbox.push({
          id: `mr-${mr.id}`,
          type: 'Review',
          title: mr.title || `MR #${mr.id}`,
          subtitle: mr.repository_id ? `repo: ${mr.repository_id}` : '',
          created_at: mr.created_at,
        });
      }

      // Spec approval items — include sha for inline approve action
      if (pendingSpecs.status === 'fulfilled') {
        for (const spec of (pendingSpecs.value || [])) {
          inbox.push({
            id: `spec-${spec.path}`,
            type: 'Spec',
            title: `Approve: ${spec.path}`,
            subtitle: spec.title || '',
            created_at: spec.updated_at,
            spec_path: spec.path,
            spec_sha: spec.sha,
          });
        }
      }

      // Gate failure items — fetched from MR gate results so we have mr_id for retry
      if (mrList.length > 0) {
        const gateChecks = await Promise.allSettled(
          mrList.map(mr => api.mrGates(mr.id).then(gates => ({ mr, gates })))
        );
        for (const result of gateChecks) {
          if (result.status !== 'fulfilled') continue;
          const { mr, gates } = result.value;
          const failedGates = (gates || []).filter(g => g.status === 'failed' || g.status === 'Failed');
          for (const gate of failedGates) {
            inbox.push({
              id: `gate-${mr.id}-${gate.id || gate.gate_id}`,
              type: 'Gate',
              title: `Gate failure: ${gate.gate_name || gate.name || 'unknown'}`,
              subtitle: mr.title ? `MR: ${mr.title}` : '',
              created_at: gate.finished_at || gate.started_at || mr.created_at,
              mr_id: mr.id,
            });
          }
        }
      }

      inbox.sort((a, b) => {
        const ta = a.created_at ? new Date(a.created_at).getTime() : 0;
        const tb = b.created_at ? new Date(b.created_at).getTime() : 0;
        return tb - ta;
      });

      items = inbox;
      error = null;
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  function markSeen(id) {
    seenIds = new Set([...seenIds, id]);
    localStorage.setItem(SEEN_KEY, JSON.stringify([...seenIds]));
  }

  async function approveSpec(item) {
    if (!item.spec_path || !item.spec_sha) return;
    actionStates = { ...actionStates, [item.id]: { loading: true } };
    try {
      await api.approveSpec(item.spec_path, item.spec_sha);
      actionStates = { ...actionStates, [item.id]: { loading: false, success: true, message: 'Approved' } };
      markSeen(item.id);
      // Reload after a short delay so the approved item disappears
      setTimeout(loadInbox, 1200);
    } catch (e) {
      actionStates = { ...actionStates, [item.id]: { loading: false, success: false, message: e.message || 'Approval failed' } };
    }
  }

  async function rejectSpec(item) {
    if (!item.spec_path) return;
    actionStates = { ...actionStates, [item.id]: { loading: true } };
    try {
      await api.revokeSpec(item.spec_path, 'Rejected from inbox');
      actionStates = { ...actionStates, [item.id]: { loading: false, success: true, message: 'Rejected' } };
      markSeen(item.id);
      setTimeout(loadInbox, 1200);
    } catch (e) {
      actionStates = { ...actionStates, [item.id]: { loading: false, success: false, message: e.message || 'Rejection failed' } };
    }
  }

  async function retryGate(item) {
    if (!item.mr_id) return;
    actionStates = { ...actionStates, [item.id]: { loading: true } };
    try {
      await api.enqueue(item.mr_id);
      actionStates = { ...actionStates, [item.id]: { loading: false, success: true, message: 'Re-queued' } };
      markSeen(item.id);
      setTimeout(loadInbox, 1200);
    } catch (e) {
      actionStates = { ...actionStates, [item.id]: { loading: false, success: false, message: e.message || 'Retry failed' } };
    }
  }

  function relativeTime(ts) {
    if (!ts) return '';
    const diff = Date.now() - new Date(ts).getTime();
    const m = Math.floor(diff / 60000);
    if (m < 1) return 'just now';
    if (m < 60) return `${m}m ago`;
    const h = Math.floor(m / 60);
    if (h < 24) return `${h}h ago`;
    return `${Math.floor(h / 24)}d ago`;
  }

  function badgeVariant(type) {
    if (type === 'Review') return 'info';
    if (type === 'Spec') return 'warning';
    if (type === 'Gate') return 'danger';
    return 'default';
  }

  onMount(() => {
    loadInbox();
    refreshInterval = setInterval(loadInbox, 60000);
  });

  onDestroy(() => {
    if (refreshInterval) clearInterval(refreshInterval);
  });
</script>

<div class="inbox">
  <div class="inbox-header">
    <div class="inbox-title-row">
      <h1 class="inbox-title">Inbox</h1>
      {#if pendingCount > 0}
        <span class="inbox-badge" aria-label="{pendingCount} pending items">{pendingCount}</span>
      {/if}
    </div>
    <Button variant="ghost" size="sm" onclick={loadInbox}>Refresh</Button>
  </div>

  {#if loading}
    <div class="inbox-list">
      {#each [1,2,3] as _}
        <Skeleton height="80px" />
      {/each}
    </div>
  {:else if error}
    <div class="inbox-error" role="alert">Error loading inbox: {error}</div>
  {:else if items.length === 0}
    <EmptyState
      title="All caught up!"
      description="No pending reviews, spec approvals, or gate failures."
    />
  {:else}
    <div class="inbox-list" role="list">
      {#each items as item (item.id)}
        {@const state = actionStates[item.id]}
        <div
          class="inbox-item"
          class:seen={seenIds.has(item.id)}
          role="listitem"
        >
          <button
            class="inbox-item-body"
            onclick={() => markSeen(item.id)}
            aria-pressed={seenIds.has(item.id)}
            aria-label="Mark as seen: {item.title}"
          >
            <div class="item-header">
              <Badge value={item.type} variant={badgeVariant(item.type)} />
              <span class="item-age">{relativeTime(item.created_at)}</span>
            </div>
            <div class="item-title">{item.title}</div>
            {#if item.subtitle}
              <div class="item-subtitle">{item.subtitle}</div>
            {/if}
          </button>

          {#if state?.message}
            <div class="action-feedback" class:success={state.success} class:failure={!state.success}>
              {state.message}
            </div>
          {/if}

          {#if item.type === 'Spec' && item.spec_path && item.spec_sha && !state?.success}
            <div class="item-actions">
              <Button
                variant="primary"
                size="sm"
                disabled={state?.loading}
                onclick={(e) => { e.stopPropagation(); approveSpec(item); }}
              >
                {state?.loading ? 'Approving…' : 'Approve'}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                disabled={state?.loading}
                onclick={(e) => { e.stopPropagation(); rejectSpec(item); }}
              >
                {state?.loading ? 'Rejecting…' : 'Reject'}
              </Button>
            </div>
          {/if}

          {#if item.type === 'Gate' && item.mr_id && !state?.success}
            <div class="item-actions">
              <Button
                variant="ghost"
                size="sm"
                disabled={state?.loading}
                onclick={(e) => { e.stopPropagation(); retryGate(item); }}
              >
                {state?.loading ? 'Retrying…' : 'Retry'}
              </Button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .inbox {
    padding: var(--space-6);
    max-width: 800px;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .inbox-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .inbox-title-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .inbox-title {
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-text);
    margin: 0;
  }

  .inbox-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 22px;
    height: 22px;
    padding: 0 var(--space-1);
    background: var(--color-primary);
    color: #fff;
    border-radius: 999px;
    font-size: var(--text-xs);
    font-weight: 700;
  }

  .inbox-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .inbox-item {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg, var(--radius));
    transition: border-color var(--transition-fast), opacity var(--transition-fast);
    overflow: hidden;
  }

  .inbox-item:hover {
    border-color: var(--color-border-strong);
  }

  .inbox-item.seen {
    opacity: 0.45;
  }

  .inbox-item-body {
    display: block;
    width: 100%;
    padding: var(--space-4);
    text-align: left;
    cursor: pointer;
    background: transparent;
    border: none;
    font-family: var(--font-body);
    color: var(--color-text);
  }

  .inbox-item-body:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .item-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-2);
  }

  .item-age {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .item-title {
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
    margin-bottom: var(--space-1);
  }

  .item-subtitle {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .item-actions {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4) var(--space-3);
    border-top: 1px solid var(--color-border);
    background: var(--color-bg);
  }

  .action-feedback {
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-4);
    font-weight: 500;
  }

  .action-feedback.success {
    color: var(--color-success, #22c55e);
  }

  .action-feedback.failure {
    color: var(--color-danger);
  }

  .inbox-error {
    color: var(--color-danger);
    font-size: var(--text-sm);
    padding: var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-danger);
    border-radius: var(--radius);
  }
</style>
