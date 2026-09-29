const RECONNECT_DELAY_MS = 3000;
const AUTH_TOKEN_KEY = 'gyre_auth_token';

function getAuthToken() {
  return localStorage.getItem(AUTH_TOKEN_KEY) || 'gyre-dev-token';
}

/** Generate a per-tab session id (random UUID) for presence tracking (HSI §7). */
function newSessionId() {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  // Fallback for environments without crypto.randomUUID (older jsdom).
  return 'sess-' + Math.random().toString(36).slice(2) + Date.now().toString(36);
}

export function createWsStore() {
  let ws = null;
  let reconnectTimer = null;
  let listeners = new Set();
  let statusListeners = new Set();
  let status = 'disconnected';

  // Per-tab identity for presence (HSI §7). Stable across reconnects so the
  // server can correlate Subscribe and UserPresence session_ids.
  const sessionId = newSessionId();
  // Messages queued while the socket is not yet authenticated.
  let sendQueue = [];
  // Last requested subscription — re-sent automatically on reconnect so the
  // client keeps receiving workspace broadcasts (including presence rebroadcasts).
  let subscription = null;

  function setStatus(s) {
    status = s;
    statusListeners.forEach((cb) => cb(s));
  }

  function rawSend(data) {
    // WebSocket.OPEN is always 1 per the WHATWG spec; compare numerically so
    // this works even where the WebSocket constructor lacks static readyState
    // constants (e.g. some test doubles).
    if (ws && ws.readyState === 1) {
      ws.send(data);
      return true;
    }
    return false;
  }

  /** Send a WsMessage object. Queues until the connection is authenticated. */
  function send(obj) {
    const data = JSON.stringify(obj);
    if (status === 'connected' && rawSend(data)) return;
    sendQueue.push(data);
  }

  function flushQueue() {
    if (sendQueue.length === 0) return;
    const pending = sendQueue;
    sendQueue = [];
    for (const data of pending) {
      if (!rawSend(data)) sendQueue.push(data);
    }
  }

  function sendSubscribe() {
    if (!subscription) return;
    send({
      type: 'Subscribe',
      scopes: [{ workspace_id: subscription.workspaceId }],
      last_seen: subscription.lastSeen ?? null,
      session_id: sessionId,
    });
  }

  /**
   * Subscribe this connection to a workspace's broadcast scope. Required to
   * receive rebroadcast UserPresence messages and to have UserPresence
   * heartbeats accepted (the server validates session_id against Subscribe).
   */
  function subscribe(workspaceId, lastSeen = null) {
    if (!workspaceId) return;
    subscription = { workspaceId, lastSeen };
    sendSubscribe();
  }

  function connect() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(`${protocol}//${window.location.host}/ws`);

    ws.onopen = () => {
      ws.send(JSON.stringify({ type: 'Auth', token: getAuthToken() }));
    };

    ws.onmessage = (event) => {
      let msg;
      try {
        msg = JSON.parse(event.data);
      } catch {
        return; // ignore malformed messages
      }
      if (msg.type === 'AuthResult') {
        const ok = msg.success;
        setStatus(ok ? 'connected' : 'auth-failed');
        if (ok) {
          // Re-establish subscription and flush any queued messages.
          sendSubscribe();
          flushQueue();
        }
      } else {
        listeners.forEach((cb) => cb(msg));
      }
    };

    ws.onclose = () => {
      if (status !== 'auth-failed') {
        setStatus('disconnected');
        scheduleReconnect();
      }
    };

    ws.onerror = () => {
      setStatus('error');
    };
  }

  function scheduleReconnect() {
    if (reconnectTimer) return;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      connect();
    }, RECONNECT_DELAY_MS);
  }

  function onMessage(cb) {
    listeners.add(cb);
    return () => listeners.delete(cb);
  }

  function onStatus(cb) {
    statusListeners.add(cb);
    cb(status);
    return () => statusListeners.delete(cb);
  }

  function destroy() {
    clearTimeout(reconnectTimer);
    listeners.clear();
    statusListeners.clear();
    ws?.close();
  }

  connect();

  return { onMessage, onStatus, destroy, send, subscribe, sessionId };
}
