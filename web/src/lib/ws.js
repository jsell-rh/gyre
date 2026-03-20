const RECONNECT_DELAY_MS = 3000;
const AUTH_TOKEN_KEY = 'gyre_auth_token';

function getAuthToken() {
  return localStorage.getItem(AUTH_TOKEN_KEY) || 'test-token';
}

export function createWsStore() {
  let ws = null;
  let reconnectTimer = null;
  let listeners = new Set();
  let statusListeners = new Set();
  let status = 'disconnected';

  function setStatus(s) {
    status = s;
    statusListeners.forEach((cb) => cb(s));
  }

  function connect() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    ws = new WebSocket(`${protocol}//${window.location.host}/ws`);

    ws.onopen = () => {
      ws.send(JSON.stringify({ type: 'Auth', token: getAuthToken() }));
    };

    ws.onmessage = (event) => {
      const msg = JSON.parse(event.data);
      if (msg.type === 'AuthResult') {
        setStatus(msg.success ? 'connected' : 'auth-failed');
      } else {
        listeners.forEach((cb) => cb(msg));
      }
    };

    ws.onclose = () => {
      setStatus('disconnected');
      scheduleReconnect();
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
    if (reconnectTimer) clearTimeout(reconnectTimer);
    listeners.clear();
    statusListeners.clear();
    ws?.close();
  }

  connect();

  return { onMessage, onStatus, destroy };
}
