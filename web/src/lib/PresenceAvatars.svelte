<script>
  function authHeaders() {
    const token = localStorage.getItem('gyre_auth_token') || 'gyre-dev-token';
    return { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' };
  }

  /**
   * PresenceAvatars — row of avatars for active workspace users.
   *
   * Spec ref: HSI §1 (Presence), ui-layout.md §1 (Status Bar presence)
   * Active = heartbeat within last 60 seconds.
   * Multiple sessions by same user collapse to single avatar + badge count.
   *
   * Props:
   *   workspaceId — string
   *   wsStore     — WebSocket store (for real-time presence updates)
   */
  let { workspaceId = '', wsStore = null } = $props();

  /** @type {Array<{user_id: string, display_name?: string, view: string, session_id: string}>} */
  let presenceList = $state([]);
  let tooltipUserId = $state(null);

  // Collapse multiple sessions by same user.
  let collapsed = $derived(collapsePresence(presenceList));

  function collapsePresence(list) {
    /** @type {Map<string, {user_id, display_name, sessions: string[], views: string[]}>} */
    const map = new Map();
    for (const entry of list) {
      if (!map.has(entry.user_id)) {
        map.set(entry.user_id, {
          user_id: entry.user_id,
          display_name: entry.display_name ?? entry.user_id,
          sessions: [],
          views: [],
        });
      }
      const u = map.get(entry.user_id);
      u.sessions.push(entry.session_id);
      if (entry.view && entry.view !== 'disconnected') {
        u.views.push(entry.view);
      }
    }
    return [...map.values()];
  }

  // Fetch initial presence on mount.
  $effect(() => {
    if (!workspaceId) return;
    let cancelled = false;

    async function fetchPresence() {
      try {
        const res = await fetch(`/api/v1/workspaces/${workspaceId}/presence`, { headers: authHeaders() });
        if (!res.ok) throw new Error(`${res.status}`);
        const data = await res.json();
        if (!cancelled && Array.isArray(data)) {
          presenceList = data;
        }
      } catch {
        // Presence endpoint may not exist yet — graceful degradation.
      }
    }

    fetchPresence();
    const interval = setInterval(fetchPresence, 30_000);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  });

  // Subscribe to WebSocket presence events.
  $effect(() => {
    if (!wsStore) return;
    const unsub = wsStore.onMessage((msg) => {
      if (!msg) return;
      if (msg.type === 'UserPresence') {
        if (msg.view === 'disconnected') {
          presenceList = presenceList.filter(
            (p) => p.session_id !== msg.session_id
          );
        } else {
          const idx = presenceList.findIndex((p) => p.session_id === msg.session_id);
          if (idx >= 0) {
            presenceList = presenceList.map((p, i) =>
              i === idx ? { ...p, view: msg.view } : p
            );
          } else {
            presenceList = [...presenceList, msg];
          }
        }
      } else if (msg.type === 'PresenceEvicted') {
        presenceList = presenceList.filter(
          (p) => p.session_id !== msg.session_id
        );
      }
    });
    return unsub;
  });

  /** Get initials from user id or display name. */
  function initials(user) {
    const name = user.display_name ?? user.user_id;
    return name
      .split(/[\s._-]+/)
      .map((part) => part[0]?.toUpperCase() ?? '')
      .slice(0, 2)
      .join('');
  }

  /** Deterministic hue from user_id for avatar color. */
  function avatarHue(userId) {
    let hash = 0;
    for (let i = 0; i < userId.length; i++) {
      hash = (hash * 31 + userId.charCodeAt(i)) | 0;
    }
    return Math.abs(hash) % 360;
  }
</script>

{#if collapsed.length > 0}
  <div class="presence-avatars" role="group" aria-label="Active users in this workspace">
    {#each collapsed as user}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="avatar-wrap"
        tabindex="0"
        aria-describedby="presence-tooltip-{user.user_id}"
        onmouseenter={() => (tooltipUserId = user.user_id)}
        onmouseleave={() => (tooltipUserId = null)}
        onfocus={() => (tooltipUserId = user.user_id)}
        onblur={() => (tooltipUserId = null)}
      >
        <div
          class="avatar"
          style="--avatar-hue: {avatarHue(user.user_id)}"
          role="img"
          aria-label="{user.display_name} — {user.views[0] ?? 'online'}"
        >
          {initials(user)}
          {#if user.sessions.length > 1}
            <span class="session-badge" aria-label="{user.sessions.length} sessions">
              {user.sessions.length}
            </span>
          {/if}
        </div>

        <div
          class="avatar-tooltip"
          class:visible={tooltipUserId === user.user_id}
          role="tooltip"
          id="presence-tooltip-{user.user_id}"
          aria-hidden={tooltipUserId !== user.user_id}
        >
          <span class="tooltip-name">{user.display_name}</span>
          {#if user.views[0]}
            <span class="tooltip-view">in {user.views[0]}</span>
          {/if}
          {#if user.sessions.length > 1}
            <span class="tooltip-sessions">{user.sessions.length} tabs open</span>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{/if}

<style>
  .presence-avatars {
    display: flex;
    align-items: center;
    gap: 0;
  }

  .avatar-wrap {
    position: relative;
    margin-left: -6px;
  }

  .avatar-wrap:first-child {
    margin-left: 0;
  }

  .avatar {
    width: 24px;
    height: 24px;
    border-radius: var(--radius-full);
    /* Deterministic color from hue */
    background: hsl(var(--avatar-hue), 55%, 35%);
    border: 2px solid var(--color-surface);
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    font-weight: 700;
    color: var(--color-text-inverse);
    cursor: default;
    position: relative;
    transition: transform var(--transition-fast);
  }

  .avatar-wrap:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-radius: var(--radius-full);
  }

  .avatar-wrap:hover .avatar,
  .avatar-wrap:focus-visible .avatar {
    transform: translateY(-2px);
    z-index: 10;
  }

  .session-badge {
    position: absolute;
    bottom: -3px;
    right: -3px;
    width: 14px;
    height: 14px;
    border-radius: var(--radius-full);
    background: var(--color-primary);
    border: 1.5px solid var(--color-surface);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: var(--text-xs);
    font-weight: 700;
    color: var(--color-text-inverse);
    line-height: 1;
  }

  .avatar-tooltip {
    position: absolute;
    bottom: calc(100% + var(--space-2));
    left: 50%;
    transform: translateX(-50%);
    z-index: 100;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg);
    padding: var(--space-2) var(--space-3);
    white-space: nowrap;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--transition-fast);
  }

  .avatar-tooltip.visible {
    opacity: 1;
    pointer-events: auto;
  }

  .tooltip-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .tooltip-view {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .tooltip-sessions {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
  }

  @media (prefers-reduced-motion: reduce) {
    .avatar-wrap, .avatar-tooltip { transition: none; }
  }
</style>
