<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';

  let { wsStatus = 'disconnected' } = $props();

  let version = $state(null);
  let loading = $state(true);

  $effect(() => {
    api.version()
      .then((data) => { version = data; loading = false; })
      .catch(() => { loading = false; });
  });

  // Relative uptime via interval update
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => { now = Date.now(); }, 60000);
    return () => clearInterval(id);
  });

  const wsStatusInfo = {
    connected:    { label: 'Connected',    color: 'var(--color-success)' },
    disconnected: { label: 'Disconnected', color: 'var(--color-text-muted)' },
    error:        { label: 'Error',        color: 'var(--color-danger)' },
    'auth-failed':{ label: 'Auth Failed',  color: 'var(--color-danger)' },
  };

  let wsInfo = $derived(wsStatusInfo[wsStatus] ?? { label: wsStatus, color: 'var(--color-text-muted)' });
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Settings</h2>
  </div>

  <div class="content">
    <!-- Server Info Card -->
    <div class="info-card">
      <div class="card-header">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
          <rect x="2" y="3" width="20" height="14" rx="2"/>
          <path d="M8 21h8M12 17v4"/>
        </svg>
        <h3>Server Information</h3>
      </div>

      {#if loading}
        <div class="skeleton-group">
          <Skeleton height="1rem" width="80%" />
          <Skeleton height="1rem" width="60%" />
          <Skeleton height="1rem" width="70%" />
        </div>
      {:else if version}
        <dl class="info-list">
          <div class="info-row">
            <dt class="info-label">Name</dt>
            <dd class="info-value">{version.name}</dd>
          </div>
          <div class="info-row">
            <dt class="info-label">Version</dt>
            <dd class="info-value mono">{version.version}</dd>
          </div>
          <div class="info-row">
            <dt class="info-label">Milestone</dt>
            <dd class="info-value">{version.milestone}</dd>
          </div>
        </dl>

        <div class="status-indicator connected">
          <div class="status-dot"></div>
          <span>Server reachable</span>
        </div>
      {:else}
        <div class="error-row">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
            <circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>
          </svg>
          <span>Could not connect to server</span>
        </div>
      {/if}
    </div>

    <!-- WebSocket Connection -->
    <div class="info-card">
      <div class="card-header">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
          <path d="M5 12.55a11 11 0 0114.08 0M1.42 9a16 16 0 0121.16 0M8.53 16.11a6 6 0 016.95 0M12 20h.01"/>
        </svg>
        <h3>WebSocket Connection</h3>
      </div>

      <div class="ws-status-row">
        <div class="ws-dot" style:background={wsInfo.color}
          class:pulse={wsStatus === 'connected'}></div>
        <div class="ws-status-text">
          <span class="ws-label" style:color={wsInfo.color}>{wsInfo.label}</span>
          <span class="ws-sublabel">Real-time event stream</span>
        </div>
      </div>
    </div>

    <!-- Environment / Configuration -->
    <div class="info-card">
      <div class="card-header">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
          <path d="M12 20h9M16.5 3.5a2.121 2.121 0 013 3L7 19l-4 1 1-4L16.5 3.5z"/>
        </svg>
        <h3>Configuration</h3>
      </div>

      <table class="config-table">
        <thead>
          <tr>
            <th>Setting</th>
            <th>Value</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td class="config-key">API Base URL</td>
            <td class="config-val mono">{window.location.origin}/api</td>
          </tr>
          <tr>
            <td class="config-key">WebSocket URL</td>
            <td class="config-val mono">{window.location.origin.replace('http', 'ws')}/ws</td>
          </tr>
          <tr>
            <td class="config-key">Dashboard Build</td>
            <td class="config-val mono">Svelte 5</td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- About -->
    <div class="info-card about-card">
      <div class="gyre-logo">
        <span class="logo-text">Gyre</span>
        <span class="logo-sub">Autonomous Development Platform</span>
      </div>
      <p class="about-desc">
        Gyre is an autonomous software development platform. Agents collaborate to write, review, and deploy code — all coordinated through the Gyre server.
      </p>
      {#if version}
        <div class="version-tag">v{version.version}</div>
      {/if}
    </div>
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .panel-header {
    display: flex;
    align-items: center;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  h2 {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    max-width: 600px;
  }

  /* Cards */
  .info-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-5) var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    transition: border-color var(--transition-fast);
  }

  .info-card:hover {
    border-color: var(--color-border-strong);
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-secondary);
  }

  h3 {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  /* Info list */
  .info-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .info-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    font-size: var(--text-sm);
  }

  .info-label {
    color: var(--color-text-muted);
    width: 8rem;
    flex-shrink: 0;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .info-value {
    color: var(--color-text-secondary);
    margin: 0;
  }

  .info-value.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .skeleton-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  /* Status indicators */
  .status-indicator {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .status-indicator.connected { color: var(--color-success); }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: currentColor;
    flex-shrink: 0;
  }

  .error-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-danger);
    font-size: var(--text-sm);
  }

  /* WebSocket status */
  .ws-status-row {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .ws-dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    flex-shrink: 0;
    transition: background var(--transition-fast);
  }

  @keyframes ws-pulse {
    0%, 100% { box-shadow: 0 0 0 0 rgba(99, 153, 61, 0.4); }
    50%       { box-shadow: 0 0 0 4px rgba(99, 153, 61, 0); }
  }

  .ws-dot.pulse {
    animation: ws-pulse 2s ease-in-out infinite;
  }

  .ws-status-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .ws-label {
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .ws-sublabel {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* Config table */
  .config-table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
  }

  .config-table th {
    text-align: left;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: var(--space-2) var(--space-3);
    border-bottom: 1px solid var(--color-border);
  }

  .config-table tbody tr {
    border-bottom: 1px solid var(--color-border);
    transition: background var(--transition-fast);
  }
  .config-table tbody tr:last-child { border-bottom: none; }
  .config-table tbody tr:hover { background: var(--color-surface-elevated); }

  .config-table td {
    padding: var(--space-2) var(--space-3);
    vertical-align: middle;
  }

  .config-key {
    color: var(--color-text-secondary);
    width: 40%;
  }

  .config-val {
    color: var(--color-text-muted);
  }

  .config-val.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  /* About card */
  .about-card {
    align-items: flex-start;
  }

  .gyre-logo {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .logo-text {
    font-family: var(--font-display);
    font-size: var(--text-2xl);
    font-weight: 700;
    color: var(--color-primary);
    letter-spacing: -0.02em;
  }

  .logo-sub {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .about-desc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.7;
    margin: 0;
  }

  .version-tag {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-2);
  }

  .mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }
</style>
