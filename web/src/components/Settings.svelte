<script>
  import { api } from '../lib/api.js';

  let { wsStatus = 'disconnected' } = $props();

  let version = $state(null);
  let loading = $state(true);

  $effect(() => {
    api.version()
      .then((data) => { version = data; loading = false; })
      .catch(() => { loading = false; });
  });
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Settings</h2>
  </div>

  <div class="content">
    <section class="card">
      <h3>Server Info</h3>
      {#if loading}
        <p class="muted">Loading…</p>
      {:else if version}
        <dl>
          <dt>Name</dt><dd>{version.name}</dd>
          <dt>Version</dt><dd>{version.version}</dd>
          <dt>Milestone</dt><dd>{version.milestone}</dd>
        </dl>
      {:else}
        <p class="error">Could not load server info.</p>
      {/if}
    </section>

    <section class="card">
      <h3>WebSocket Connection</h3>
      <div class="ws-status">
        <span class="dot {wsStatus}"></span>
        <span class="ws-label">{wsStatus}</span>
      </div>
    </section>
  </div>
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex; align-items: center;
    padding: 1rem 1.25rem; border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  h2 { margin: 0; font-size: 1rem; font-weight: 600; color: var(--text); }
  h3 { margin: 0 0 0.75rem; font-size: 0.88rem; font-weight: 600; color: var(--text-muted); }

  .content { flex: 1; overflow-y: auto; padding: 1.25rem; display: flex; flex-direction: column; gap: 1rem; max-width: 500px; }

  .card {
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 6px; padding: 1rem 1.25rem;
  }

  dl { display: grid; grid-template-columns: 8rem 1fr; gap: 0.4rem 0.75rem; font-size: 0.85rem; }
  dt { color: var(--text-dim); }
  dd { margin: 0; color: var(--text-muted); }

  .ws-status { display: flex; align-items: center; gap: 0.6rem; }

  .dot {
    width: 10px; height: 10px; border-radius: 50%; flex-shrink: 0;
    background: var(--text-dim);
  }
  .dot.connected    { background: #4ade80; box-shadow: 0 0 6px #22c55e88; }
  .dot.disconnected { background: #f97316; }
  .dot.error        { background: #f87171; }
  .dot.auth-failed  { background: #f87171; }

  .ws-label { font-size: 0.85rem; color: var(--text-muted); }

  .muted { color: var(--text-dim); font-size: 0.85rem; font-style: italic; }
  .error { color: #f87171; font-size: 0.85rem; }
</style>
