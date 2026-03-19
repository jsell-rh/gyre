<script>
  const version = '0.1.0';
  let connectionStatus = $state('disconnected');

  $effect(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;
    let ws;

    function connect() {
      ws = new WebSocket(wsUrl);

      ws.onopen = () => {
        ws.send(JSON.stringify({ type: 'Auth', token: 'gyre-dev-token' }));
      };

      ws.onmessage = (event) => {
        const msg = JSON.parse(event.data);
        if (msg.type === 'AuthResult') {
          connectionStatus = msg.success ? 'connected' : 'auth-failed';
        }
      };

      ws.onclose = () => {
        connectionStatus = 'disconnected';
      };

      ws.onerror = () => {
        connectionStatus = 'error';
      };
    }

    connect();
    return () => ws?.close();
  });
</script>

<main>
  <h1>Gyre</h1>
  <p class="status">Connection: <span class={connectionStatus}>{connectionStatus}</span></p>
  <p class="version">v{version}</p>
</main>

<style>
  main {
    font-family: system-ui, sans-serif;
    max-width: 640px;
    margin: 4rem auto;
    padding: 0 1rem;
    text-align: center;
  }

  h1 {
    font-size: 3rem;
    font-weight: 700;
    margin-bottom: 1rem;
  }

  .status {
    font-size: 1.1rem;
    color: #555;
  }

  .connected { color: #22c55e; }
  .disconnected { color: #f97316; }
  .error { color: #ef4444; }
  .auth-failed { color: #ef4444; }

  .version {
    margin-top: 2rem;
    color: #999;
    font-size: 0.9rem;
  }
</style>
