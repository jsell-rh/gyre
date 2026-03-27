<script>
  import { t, locale } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';

  let { wsStatus = 'disconnected' } = $props();

  let version = $state(null);
  let loading = $state(true);

  function fetchVersion() {
    loading = true;
    version = null;
    api.version()
      .then((data) => { version = data; loading = false; })
      .catch(() => { loading = false; });
  }

  $effect(() => {
    fetchVersion();
  });

  // Relative uptime via interval update
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => { now = Date.now(); }, 60000);
    return () => clearInterval(id);
  });

  const supportedLocales = [
    { code: 'en', label: 'English' },
  ];

  let wsInfo = $derived((() => {
    const map = {
      connected:    { label: $t('settings.websocket.status.connected'),    color: 'var(--color-success)' },
      disconnected: { label: $t('settings.websocket.status.disconnected'), color: 'var(--color-text-muted)' },
      error:        { label: $t('settings.websocket.status.error'),        color: 'var(--color-danger)' },
      'auth-failed':{ label: $t('settings.websocket.status.auth_failed'),  color: 'var(--color-danger)' },
    };
    return map[wsStatus] ?? { label: wsStatus, color: 'var(--color-text-muted)' };
  })());
</script>

<div class="panel">
  <div class="panel-header">
    <h2>{$t('settings.title')}</h2>
  </div>

  <div class="content">
    <!-- Server Info Card -->
    <section class="info-card" aria-labelledby="settings-server-info">
      <div class="card-header">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
          <rect x="2" y="3" width="20" height="14" rx="2"/>
          <path d="M8 21h8M12 17v4"/>
        </svg>
        <h3 id="settings-server-info">{$t('settings.server_info.title')}</h3>
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
            <dt class="info-label">{$t('settings.server_info.name')}</dt>
            <dd class="info-value">{version.name}</dd>
          </div>
          <div class="info-row">
            <dt class="info-label">{$t('settings.server_info.version')}</dt>
            <dd class="info-value mono">{version.version}</dd>
          </div>
          <div class="info-row">
            <dt class="info-label">{$t('settings.server_info.milestone')}</dt>
            <dd class="info-value">{version.milestone}</dd>
          </div>
        </dl>

        <div class="status-indicator connected">
          <div class="status-dot"></div>
          <span>{$t('settings.server_info.reachable')}</span>
        </div>
      {:else}
        <div class="error-row">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
            <circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>
          </svg>
          <span>{$t('settings.server_info.unreachable')}</span>
          <button class="btn-retry" onclick={() => { fetchVersion(); }}>Retry</button>
        </div>
      {/if}
    </section>

    <!-- WebSocket Connection -->
    <section class="info-card" aria-labelledby="settings-websocket">
      <div class="card-header">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
          <path d="M5 12.55a11 11 0 0114.08 0M1.42 9a16 16 0 0121.16 0M8.53 16.11a6 6 0 016.95 0M12 20h.01"/>
        </svg>
        <h3 id="settings-websocket">{$t('settings.websocket.title')}</h3>
      </div>

      <div class="ws-status-row">
        <div class="ws-dot" style:background={wsInfo.color}
          class:pulse={wsStatus === 'connected'}></div>
        <div class="ws-status-text">
          <span class="ws-label" style:color={wsInfo.color}>{wsInfo.label}</span>
          <span class="ws-sublabel">{$t('settings.websocket.sublabel')}</span>
        </div>
      </div>
    </section>

    <!-- Environment / Configuration -->
    <section class="info-card" aria-labelledby="settings-config">
      <div class="card-header">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
          <path d="M12 20h9M16.5 3.5a2.121 2.121 0 013 3L7 19l-4 1 1-4L16.5 3.5z"/>
        </svg>
        <h3 id="settings-config">{$t('settings.config.title')}</h3>
      </div>

      <table class="config-table">
        <thead>
          <tr>
            <th>{$t('settings.config.setting')}</th>
            <th>{$t('settings.config.value')}</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td class="config-key">{$t('settings.config.api_base_url')}</td>
            <td class="config-val mono">{window.location.origin}/api</td>
          </tr>
          <tr>
            <td class="config-key">{$t('settings.config.ws_url')}</td>
            <td class="config-val mono">{window.location.origin.replace('http', 'ws')}/ws</td>
          </tr>
          <tr>
            <td class="config-key">{$t('settings.config.dashboard_build')}</td>
            <td class="config-val mono">Svelte 5</td>
          </tr>
        </tbody>
      </table>
    </section>

    <!-- Language -->
    <section class="info-card" aria-labelledby="settings-language">
      <div class="card-header">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
          <circle cx="12" cy="12" r="10"/><path d="M2 12h20M12 2a15.3 15.3 0 014 10 15.3 15.3 0 01-4 10 15.3 15.3 0 01-4-10 15.3 15.3 0 014-10z"/>
        </svg>
        <h3 id="settings-language">{$t('settings.language.title')}</h3>
      </div>
      <div class="lang-row">
        <label class="lang-label" for="lang-select">{$t('settings.language.label')}</label>
        <select id="lang-select" class="lang-select" bind:value={$locale}>
          {#each supportedLocales as loc}
            <option value={loc.code}>{loc.label}</option>
          {/each}
        </select>
      </div>
    </section>

    <!-- About -->
    <section class="info-card about-card" aria-labelledby="settings-about">
      <h3 id="settings-about" class="sr-only">{$t('settings.about.title', { default: 'About' })}</h3>
      <div class="gyre-logo">
        <span class="logo-text" aria-hidden="true">Gyre</span>
        <span class="logo-sub">{$t('settings.about.tagline')}</span>
      </div>
      <p class="about-desc">
        Gyre is an autonomous software development platform. Agents collaborate to write, review, and deploy code — all coordinated through the Gyre server.
      </p>
      {#if version}
        <div class="version-tag">v{version.version}</div>
      {/if}
    </section>
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
    0%, 100% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--color-success) 40%, transparent); }
    50%       { box-shadow: 0 0 0 4px color-mix(in srgb, var(--color-success) 0%, transparent); }
  }

  .ws-dot.pulse {
    animation: ws-pulse 2s ease-in-out infinite;
  }

  .ws-status-text {
    display: flex;
    flex-direction: column;
    gap: var(--space-0, 2px);
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
    padding: var(--space-3) var(--space-4);
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

  .lang-row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
  }

  .lang-label {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    min-width: 10rem;
  }

  .lang-select {
    background: var(--color-surface-elevated);
    color: var(--color-text);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    padding: var(--space-1) var(--space-3);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .lang-select:focus:not(:focus-visible) {
    outline: none;
  }
  .lang-select:focus-visible {
    outline: 2px solid var(--color-focus, #4db0ff);
    outline-offset: 2px;
  }

  .btn-retry {
    margin-left: var(--space-2);
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-size: var(--text-xs);
    cursor: pointer;
  }
  .btn-retry:hover { background: var(--color-surface-hover); }
  .btn-retry:focus-visible { outline: 2px solid var(--color-focus, #4db0ff); outline-offset: 2px; }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  @media (prefers-reduced-motion: reduce) {
    .ws-dot.pulse { animation: none; }
  }
</style>
