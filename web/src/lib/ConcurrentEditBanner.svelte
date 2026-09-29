<script>
  import { t } from 'svelte-i18n';
  import { api } from './api.js';

  /**
   * ConcurrentEditBanner — warns when another user is editing the same spec
   * (HSI §7 Conflict Prevention).
   *
   * Detects concurrent editors from the presence map: fetches the initial state
   * via GET /workspaces/:id/presence and stays live by subscribing to WebSocket
   * `UserPresence` / `PresenceEvicted` messages. An "editor" is a presence entry
   * whose `editing_entity` equals `spec:<specPath>`, excluding this tab (matched
   * by session_id) and this user (matched by user_id when known).
   *
   * Props:
   *   specPath      — string — spec path being edited (unprefixed, as sent to save)
   *   workspaceId   — string — workspace to query presence for
   *   wsStore       — WebSocket store ({ onMessage, sessionId }) for live updates
   *   selfUserId    — string | null — current user id, to exclude own sessions
   */
  let { specPath = '', workspaceId = '', wsStore = null, selfUserId = null } = $props();

  let target = $derived(specPath ? `spec:${specPath}` : null);
  let selfSessionId = $derived(wsStore?.sessionId ?? null);

  /** @type {Map<string, {user_id: string, editing_entity: string | null}>} */
  let entries = $state(new Map());

  // Other users currently editing this exact spec.
  let others = $derived(
    [...entries.values()].filter(
      (e) => e.editing_entity === target
    )
  );
  let otherNames = $derived([...new Set(others.map((e) => e.user_id))]);

  function isSelf(entry) {
    if (selfSessionId && entry.session_id === selfSessionId) return true;
    if (selfUserId && entry.user_id === selfUserId) return true;
    return false;
  }

  // Initial presence fetch when the editor opens.
  $effect(() => {
    if (!workspaceId || !target) return;
    let cancelled = false;
    (async () => {
      try {
        const data = await api.workspacePresence(workspaceId);
        if (cancelled || !Array.isArray(data)) return;
        const next = new Map();
        for (const e of data) {
          if (isSelf(e)) continue;
          next.set(e.session_id, {
            session_id: e.session_id,
            user_id: e.user_id,
            editing_entity: e.editing_entity ?? null,
          });
        }
        entries = next;
      } catch {
        /* presence endpoint unavailable — degrade to no banner */
      }
    })();
    return () => {
      cancelled = true;
    };
  });

  // Live presence updates over the WebSocket.
  $effect(() => {
    if (!wsStore?.onMessage) return;
    const unsub = wsStore.onMessage((msg) => {
      if (!msg) return;
      if (msg.type === 'UserPresence') {
        if (isSelf(msg)) return;
        const next = new Map(entries);
        if (msg.view === 'disconnected') {
          next.delete(msg.session_id);
        } else {
          next.set(msg.session_id, {
            session_id: msg.session_id,
            user_id: msg.user_id,
            editing_entity: msg.editing_entity ?? null,
          });
        }
        entries = next;
      } else if (msg.type === 'PresenceEvicted') {
        if (entries.has(msg.session_id)) {
          const next = new Map(entries);
          next.delete(msg.session_id);
          entries = next;
        }
      }
    });
    return unsub;
  });
</script>

{#if otherNames.length > 0}
  <div class="concurrent-edit-banner" role="alert" data-testid="concurrent-edit-banner">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
      <path d="M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0z"/>
      <line x1="12" y1="9" x2="12" y2="13"/><line x1="12" y1="17" x2="12.01" y2="17"/>
    </svg>
    <span class="concurrent-edit-text">
      {#if otherNames.length === 1}
        {$t('concurrent_edit.warning_one', { values: { user: otherNames[0] } })}
      {:else}
        {$t('concurrent_edit.warning_many', { values: { users: otherNames.join(', ') } })}
      {/if}
    </span>
  </div>
{/if}

<style>
  .concurrent-edit-banner {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    margin-bottom: 0.5rem;
    background: rgba(230, 160, 30, 0.14);
    border: 1px solid rgba(230, 160, 30, 0.45);
    border-radius: 6px;
    color: var(--text, #7a5200);
    font-size: 0.85rem;
  }
  .concurrent-edit-text {
    line-height: 1.3;
  }
</style>
