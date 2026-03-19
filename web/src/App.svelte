<script>
  const version = '0.1.0';
  let connectionStatus = $state('disconnected');
  let events = $state([]);

  function formatTs(ts) {
    return new Date(ts).toLocaleTimeString();
  }

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
        } else if (msg.type === 'ActivityEvent') {
          events = [msg, ...events].slice(0, 200);
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

  <section class="activity">
    <h2>Activity Feed</h2>
    {#if events.length === 0}
      <p class="empty">No activity yet.</p>
    {:else}
      <ul>
        {#each events as e (e.event_id)}
          <li>
            <span class="ts">{formatTs(e.timestamp)}</span>
            <span class="badge">{e.event_type}</span>
            <span class="agent">{e.agent_id}</span>
            <span class="desc">{e.description}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</main>

<style>
  main {
    font-family: system-ui, sans-serif;
    max-width: 800px;
    margin: 2rem auto;
    padding: 0 1rem;
  }

  h1 {
    font-size: 2.5rem;
    font-weight: 700;
    margin-bottom: 0.5rem;
    text-align: center;
  }

  .status {
    font-size: 1.1rem;
    color: #555;
    text-align: center;
  }

  .connected { color: #22c55e; }
  .disconnected { color: #f97316; }
  .error { color: #ef4444; }
  .auth-failed { color: #ef4444; }

  .version {
    text-align: center;
    color: #999;
    font-size: 0.9rem;
    margin-bottom: 2rem;
  }

  .activity h2 {
    font-size: 1.25rem;
    font-weight: 600;
    margin-bottom: 0.75rem;
    border-bottom: 1px solid #e5e7eb;
    padding-bottom: 0.5rem;
  }

  .empty {
    color: #999;
    font-style: italic;
  }

  ul {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    max-height: 60vh;
    overflow-y: auto;
  }

  li {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    padding: 0.4rem 0.6rem;
    background: #f9fafb;
    border-radius: 4px;
    font-size: 0.9rem;
  }

  .ts {
    color: #999;
    font-size: 0.8rem;
    white-space: nowrap;
  }

  .badge {
    background: #3b82f6;
    color: white;
    padding: 0.1rem 0.4rem;
    border-radius: 3px;
    font-size: 0.75rem;
    font-weight: 600;
    white-space: nowrap;
  }

  .agent {
    color: #6366f1;
    font-weight: 500;
    white-space: nowrap;
  }

  .desc {
    color: #374151;
    flex: 1;
  }
</style>
