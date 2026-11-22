<script>
  import { api } from './api.js';

  /**
   * TTY attach — the human window into the Ralph loop
   * (activity-dashboard.md: "attach to a TTY if needed").
   *
   * Opens the authenticated /ws/agents/:id/tty socket (the URL comes from
   * api.agentTtyUrl — this component is its consumer) and displays the
   * replayed buffer, then live output, as a terminal view. Mounted from
   * agent surfaces (detail panel, activity entries referencing a running
   * agent).
   */
  let { agentId } = $props();

  const AUTH_TOKEN_KEY = 'gyre_auth_token';
  const MAX_LINES = 500;

  let attached = $state(false);
  let lines = $state([]);
  let status = $state('idle'); // idle | connecting | open | closed | auth-failed
  let socket = null;

  function getAuthToken() {
    return localStorage.getItem(AUTH_TOKEN_KEY) || 'gyre-dev-token';
  }

  function appendLine(text) {
    lines = [...lines, text].slice(-MAX_LINES);
  }

  function detach() {
    if (socket) {
      socket.onclose = null;
      socket.close();
      socket = null;
    }
    attached = false;
    status = 'closed';
  }

  function attach() {
    detach();
    status = 'connecting';
    lines = [];
    attached = true;
    // The socket URL is consumed from api.agentTtyUrl — single source of
    // truth for the /ws/agents/:id/tty endpoint shape.
    const url = api.agentTtyUrl(agentId);
    socket = new WebSocket(url);
    socket.onopen = () => {
      socket.send(JSON.stringify({ type: 'Auth', token: getAuthToken() }));
    };
    socket.onmessage = (event) => {
      // Control frame: the auth verdict. Everything else is a log line
      // (banner, replayed buffer, live output) — displayed in order.
      let msg = null;
      try {
        msg = JSON.parse(event.data);
      } catch {
        msg = null;
      }
      if (msg && typeof msg === 'object' && msg.type === 'AuthResult') {
        if (!msg.success) {
          // Report the failure in the view and stop using the socket —
          // closing it must not also hide the failure (the viewer stays
          // mounted with the error line as its last output).
          appendLine('[tty] authentication failed');
          status = 'auth-failed';
          if (socket) {
            socket.onclose = null;
            socket.close();
            socket = null;
          }
          return;
        }
        status = 'open';
        return;
      }
      appendLine(String(event.data).replace(/\r?\n$/, ''));
    };
    socket.onclose = () => {
      if (attached) {
        status = 'closed';
        appendLine('[tty] disconnected');
      }
      attached = false;
      socket = null;
    };
    socket.onerror = () => {
      if (status === 'connecting') status = 'closed';
    };
  }

  $effect(() => {
    // Agent switch while mounted: detach the old stream.
    void agentId;
    return () => detach();
  });
</script>

<div class="agent-tty">
  <div class="agent-tty-header">
    {#if attached && (status === 'open' || status === 'connecting')}
      <button class="agent-tty-btn agent-tty-detach" onclick={detach}>Detach TTY</button>
    {:else}
      <button class="agent-tty-btn" onclick={attach}>Attach TTY</button>
    {/if}
    {#if attached}
      <span class="agent-tty-status agent-tty-status-{status}">
        {status === 'connecting' ? 'connecting…' : status}
      </span>
    {/if}
  </div>
  {#if attached}
    <pre class="agent-tty-view" aria-label="agent tty output">{lines.join('\n')}</pre>
  {/if}
</div>

<style>
  .agent-tty {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .agent-tty-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .agent-tty-btn {
    font: inherit;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: var(--radius-sm, 4px);
    border: 1px solid var(--color-border, #ccc);
    background: var(--color-surface, #fafafa);
    cursor: pointer;
  }
  .agent-tty-btn:hover {
    background: var(--color-surface-elevated, #f0f0f0);
  }
  .agent-tty-detach {
    color: var(--color-danger, #b00);
  }
  .agent-tty-status {
    font-size: 10px;
    color: var(--color-text-muted, #666);
  }
  .agent-tty-status-open {
    color: var(--color-success, #080);
  }
  .agent-tty-status-auth-failed {
    color: var(--color-danger, #b00);
  }
  .agent-tty-view {
    margin: 0;
    max-height: 260px;
    overflow-y: auto;
    background: var(--color-surface-inverse, #111);
    color: var(--color-text-inverse, #eee);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    line-height: 1.45;
    padding: 8px;
    border-radius: var(--radius-sm, 4px);
    white-space: pre-wrap;
    word-break: break-all;
  }
</style>
