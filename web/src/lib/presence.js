/**
 * Presence helpers for concurrent spec editing (HSI §7 Conflict Prevention).
 */

/**
 * Send a UserPresence heartbeat announcing (or clearing) the spec the user is
 * actively editing. The server derives the real user_id from the authenticated
 * connection — the payload user_id is ignored — but the field must be present
 * for the message to deserialize, so a placeholder is sent.
 *
 * @param {{ send: Function, sessionId: string } | null} wsStore
 * @param {object} opts
 * @param {string} opts.workspaceId — workspace scope for the presence entry
 * @param {string|null} [opts.editingEntity] — e.g. "spec:specs/system/auth.md";
 *        omit/null when the user stops editing (banner clears for others)
 * @param {string} [opts.view] — current view label (e.g. "specs")
 */
export function sendEditingPresence(wsStore, { workspaceId, editingEntity = null, view = 'specs' }) {
  if (!wsStore || typeof wsStore.send !== 'function' || !wsStore.sessionId || !workspaceId) {
    return;
  }
  const msg = {
    type: 'UserPresence',
    user_id: 'self', // server overrides with the authenticated identity
    session_id: wsStore.sessionId,
    workspace_id: workspaceId,
    view,
    timestamp: Date.now(),
  };
  // Absent editing_entity => server stores None (user is not editing a spec).
  if (editingEntity) msg.editing_entity = editingEntity;
  wsStore.send(msg);
}

/**
 * Presence heartbeat manager (HSI §1 Presence — liveness legs).
 *
 * Spec: presence updates are sent immediately after the WebSocket connection
 * is established, then on BOTH a 30-second timer AND on view changes,
 * debounced to at most one update per 5 seconds; `view: "disconnected"` is
 * sent on `beforeunload`. The server evicts presence entries after 60 idle
 * seconds, so without the heartbeat an active editor vanishes from other
 * users' presence views after one interval.
 *
 * All legs are a conjunctive contract — see specs/prompts/implementation.md
 * item 161: send-on-connect, periodic timer, view-change re-send, and
 * disconnect-on-unload must each be implemented and tested. Eviction leg: a
 * targeted PresenceEvicted naming this tab's session_id stops heartbeating
 * for that tab (HSI §1 5-session cap / idle sweep).
 *
 * @param {{ send: Function, sessionId: string, onStatus: Function, onMessage?: Function } | null} wsStore
 * @param {object} opts
 * @param {() => string | null} opts.getWorkspaceId — current workspace scope;
 *        when null the heartbeat pauses (no workspace = no presence)
 * @param {() => string} opts.getView — current view label (e.g. "inbox", "specs")
 * @param {() => string | null} [opts.getEditingEntity] — entity the user is
 *        actively editing (e.g. "spec:specs/system/auth.md"), null otherwise.
 *        Heartbeats re-send the CURRENT value so they never clobber a live
 *        concurrent-editing announcement.
 * @returns {{ notifyViewChange: Function, sendNow: Function, destroy: Function }}
 */
export function createPresenceHeartbeat(wsStore, opts) {
  const HEARTBEAT_MS = 30_000;
  const DEBOUNCE_MS = 5_000;

  if (!wsStore || typeof wsStore.send !== 'function') {
    return { notifyViewChange: () => {}, sendNow: () => {}, destroy: () => {} };
  }

  const { getWorkspaceId, getView, getEditingEntity = () => null } = opts;
  let lastSentAt = 0;
  let timer = null;
  let alive = true;
  // Set when the server evicts THIS tab's session (PresenceEvicted with our
  // session_id, HSI §1). The spec: "the client checks if the session_id
  // matches its own tab and stops heartbeating only for that tab." Without
  // this the evicted tab keeps re-inserting its presence entry every 30s,
  // fighting the server's 5-session cap.
  let evicted = false;

  function sendHeartbeat(view) {
    if (evicted) return; // server dropped this session — stay dark
    const workspaceId = getWorkspaceId();
    if (!workspaceId) return; // no workspace scope — nothing to announce
    sendEditingPresence(wsStore, {
      workspaceId,
      editingEntity: getEditingEntity(),
      view,
    });
    lastSentAt = Date.now();
  }

  /** Send immediately unless within the 5s debounce window. */
  function sendDebounced(view) {
    if (Date.now() - lastSentAt < DEBOUNCE_MS) return;
    sendHeartbeat(view);
  }

  function currentView() {
    return getView();
  }

  // Leg 1: send immediately after the connection is established (also covers
  // reconnects, where the server may have idle-evicted our entry).
  const unsubStatus = wsStore.onStatus?.((status) => {
    if (status === 'connected') sendDebounced(currentView());
  });

  // HSI §1 (5-session cap / idle eviction): a targeted PresenceEvicted naming
  // OUR session means the server has dropped this tab from the presence map.
  // Stop heartbeating for this tab so it does not re-insert itself. A fresh
  // Subscribe (new tab) or heartbeat state is not revived here — eviction is
  // terminal for this session, matching the spec's "stops heartbeating only
  // for that tab".
  const unsubMessage = wsStore.onMessage?.((msg) => {
    if (msg?.type === 'PresenceEvicted' && msg.session_id === wsStore.sessionId) {
      evicted = true;
      alive = false;
      clearInterval(timer);
      timer = null;
    }
  });

  // Leg 2: 30-second periodic heartbeat.
  timer = setInterval(() => {
    if (!alive) return;
    sendHeartbeat(currentView());
  }, HEARTBEAT_MS);

  // Leg 4: graceful disconnect on unload (the connection is about to die with
  // the page; the server would otherwise keep the entry until idle eviction).
  const onBeforeUnload = () => sendHeartbeat('disconnected');
  window.addEventListener('beforeunload', onBeforeUnload);

  return {
    /** Leg 3: view changed (sidebar nav click or scope transition) — re-send,
     *  debounced to at most one update per 5 seconds. */
    notifyViewChange() {
      sendDebounced(currentView());
    },
    /** Force an immediate heartbeat, bypassing the debounce. */
    sendNow() {
      sendHeartbeat(currentView());
    },
    destroy() {
      alive = false;
      clearInterval(timer);
      window.removeEventListener('beforeunload', onBeforeUnload);
      unsubStatus?.();
      unsubMessage?.();
    },
  };
}

/** Build the presence editing_entity token for a spec path. */
export function specEditingEntity(specPath) {
  return specPath ? `spec:${specPath}` : null;
}
