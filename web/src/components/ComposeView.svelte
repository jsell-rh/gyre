<script>
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let composeId = $state('');
  let statusAgents = $state([]);
  let statusLoading = $state(false);
  let statusError = $state(null);

  let yamlInput = $state('');
  let applyLoading = $state(false);
  let applyError = $state(null);
  let applyResult = $state(null);

  let teardownLoading = $state(false);

  let jsonStatus = $state(null);
  let jsonError = $state('');

  $effect(() => {
    if (!yamlInput.trim()) { jsonStatus = null; return; }
    const t = setTimeout(() => {
      try { JSON.parse(yamlInput); jsonStatus = 'valid'; jsonError = ''; }
      catch(e) { jsonStatus = 'invalid'; jsonError = e.message; }
    }, 400);
    return () => clearTimeout(t);
  });

  const placeholderText = `{
  "version": "1",
  "workspace_id": "...",
  "repo_id": "...",
  "agents": [
    {
      "name": "worker-1",
      "task": "Implement feature X"
    }
  ]
}`;

  async function applyCompose() {
    if (!yamlInput.trim()) { applyError = 'Compose spec is required.'; return; }
    applyLoading = true;
    applyError = null;
    applyResult = null;
    try {
      const spec = JSON.parse(yamlInput);
      applyResult = await api.composeApply(spec);
      composeId = applyResult.compose_id;
      await loadStatus();
      toastSuccess(`Compose applied. Session: ${applyResult.compose_id.slice(0, 8)}…`);
    } catch (e) {
      applyError = e.message;
      toastError(e.message);
    } finally {
      applyLoading = false;
    }
  }

  async function loadStatus() {
    if (!composeId.trim()) return;
    statusLoading = true;
    statusError = null;
    try {
      const res = await api.composeStatus(composeId);
      statusAgents = res.agents ?? [];
    } catch (e) {
      statusError = e.message;
      statusAgents = [];
    } finally {
      statusLoading = false;
    }
  }

  async function doTeardown() {
    if (!composeId.trim()) return;
    teardownLoading = true;
    try {
      await api.composeTeardown(composeId);
      statusAgents = [];
      applyResult = null;
      composeId = '';
      toastSuccess('Compose session torn down.');
    } catch (e) {
      toastError(e.message);
    } finally {
      teardownLoading = false;
    }
  }
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Agent Compose</h2>
  </div>

  <div class="scroll">
    <!-- Apply section -->
    <div class="section-card">
      <div class="section-header">
        <h3 class="section-title">Apply Compose Spec</h3>
        <p class="section-hint" id="compose-spec-hint">Paste a JSON agent-compose spec. YAML not supported in browser.</p>
      </div>

      <textarea
        class="spec-editor"
        bind:value={yamlInput}
        placeholder={placeholderText}
        rows="10"
        spellcheck="false"
        aria-label="Spec content editor"
        aria-describedby="compose-spec-hint"
      ></textarea>

      {#if jsonStatus === 'valid'}
        <p class="json-hint valid">Valid JSON</p>
      {:else if jsonStatus === 'invalid'}
        <p class="json-hint invalid">{jsonError}</p>
      {/if}

      {#if applyError}
        <div class="form-error" role="alert">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
            <circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>
          </svg>
          {applyError}
        </div>
      {/if}

      <div class="btn-row">
        <button class="primary-btn" onclick={applyCompose} disabled={applyLoading || !yamlInput.trim()} aria-busy={applyLoading}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
            <path d="M5 12h14M12 5l7 7-7 7"/>
          </svg>
          {applyLoading ? 'Applying…' : 'Apply Compose'}
        </button>
      </div>

      {#if applyResult}
        <div class="success-banner" role="status" aria-live="polite">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
            <path d="M20 6L9 17l-5-5"/>
          </svg>
          <div>
            <div class="success-title">Compose applied successfully</div>
            <div class="success-detail">Session ID: <code class="session-id">{applyResult.compose_id}</code></div>
          </div>
        </div>
      {/if}
    </div>

    <!-- Status section -->
    <div class="section-card" aria-busy={statusLoading}>
      <div class="section-header">
        <h3 class="section-title">Compose Status</h3>
      </div>

      <div class="status-controls">
        <label for="compose-session-id" class="sr-only">Compose session ID</label>
        <input
          id="compose-session-id"
          class="id-input"
          bind:value={composeId}
          placeholder="Compose session ID…"
        />
        <button class="secondary-btn" onclick={loadStatus} disabled={statusLoading || !composeId.trim()} aria-busy={statusLoading}>
          {statusLoading ? 'Loading…' : 'Refresh'}
        </button>
        <button class="danger-btn" onclick={doTeardown} disabled={teardownLoading || !composeId.trim()} aria-busy={teardownLoading}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
            <path d="M18 6L6 18M6 6l12 12"/>
          </svg>
          {teardownLoading ? 'Stopping…' : 'Teardown'}
        </button>
      </div>

      {#if statusError}
        <div class="form-error" role="alert">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14">
            <circle cx="12" cy="12" r="10"/><path d="M12 8v4M12 16h.01"/>
          </svg>
          {statusError}
        </div>
      {:else if !composeId.trim()}
        <!-- no compose id yet -->
      {:else if statusLoading}
        <div class="loading-row">
          <div class="spinner"></div>
          <span>Loading agents…</span>
        </div>
      {:else if statusAgents.length === 0}
        <EmptyState
          title="No agents in session"
          description="Apply a compose spec or refresh to see agents."
        />
      {:else}
        <!-- Agent tree visualization -->
        <div class="agent-tree">
          <div class="tree-header">
            <span class="tree-count">{statusAgents.length} agents</span>
          </div>
          <div class="tree-nodes">
            {#each statusAgents as agent (agent.agent_id)}
              <div class="agent-node">
                <div class="node-indicator">
                  <div class="node-dot status-{agent.status}"></div>
                  <div class="node-line"></div>
                </div>
                <div class="node-body">
                  <div class="node-name">{agent.name}</div>
                  <div class="node-meta">
                    <Badge value={agent.status} />
                    <code class="node-id">{agent.agent_id.slice(0, 8)}</code>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        </div>
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

  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
    max-width: 800px;
  }

  .section-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: var(--space-6);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .section-header {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-base);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
  }

  .section-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  /* Editor */
  .spec-editor {
    width: 100%;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    padding: var(--space-3) var(--space-4);
    resize: vertical;
    min-height: 160px;
    line-height: 1.6;
    transition: border-color var(--transition-fast);
  }
  .spec-editor:focus:not(:focus-visible) {
    outline: none;
    border-color: var(--color-link);
  }
  .spec-editor:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* Buttons */
  .btn-row {
    display: flex;
    gap: var(--space-3);
  }

  .primary-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-primary);
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-inverse);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-4);
    font-family: var(--font-body);
    font-weight: 500;
    transition: background var(--transition-fast);
  }
  .primary-btn:hover { background: var(--color-primary-hover); }
  .primary-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .primary-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .secondary-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }
  .secondary-btn:hover:not(:disabled) { border-color: var(--color-border-strong); color: var(--color-text); }
  .secondary-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .secondary-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  .danger-btn {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-danger);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    transition: background var(--transition-fast);
  }
  .danger-btn:hover:not(:disabled) { background: color-mix(in srgb, var(--color-danger) 20%, transparent); }
  .danger-btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .danger-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; }

  /* Status controls */
  .status-controls {
    display: flex;
    gap: var(--space-3);
    align-items: center;
  }

  .id-input {
    flex: 1;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text);
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    padding: var(--space-2) var(--space-3);
    transition: border-color var(--transition-fast);
  }
  .id-input:focus:not(:focus-visible) {
    outline: none;
    border-color: var(--color-link);
  }
  .id-input:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* Form error */
  .form-error {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-danger);
    font-size: var(--text-sm);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 20%, transparent);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
  }

  /* Success banner */
  .success-banner {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-success) 30%, transparent);
    border-radius: var(--radius);
    padding: var(--space-3) var(--space-4);
  }

  .success-title {
    font-weight: 600;
    font-size: var(--text-sm);
    color: var(--color-success);
  }

  .success-detail {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin-top: var(--space-1);
  }

  .session-id {
    font-family: var(--font-mono);
    color: var(--color-text);
  }

  /* Loading */
  .loading-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-link);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  /* Agent tree */
  .agent-tree {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .tree-header {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: 600;
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }

  .tree-nodes {
    display: flex;
    flex-direction: column;
    gap: 0;
  }

  .agent-node {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
  }

  .node-indicator {
    display: flex;
    flex-direction: column;
    align-items: center;
    flex-shrink: 0;
    width: 16px;
  }

  .node-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    margin-top: var(--space-3);
    flex-shrink: 0;
    background: var(--color-border-strong);
  }

  .node-dot.status-active  { background: var(--color-success); }
  .node-dot.status-idle    { background: var(--color-text-muted); }
  .node-dot.status-blocked { background: var(--color-blocked); }
  .node-dot.status-error   { background: var(--color-danger); }
  .node-dot.status-dead    { background: var(--color-text-muted); }

  .node-line {
    width: 2px;
    flex: 1;
    min-height: var(--space-4);
    background: var(--color-border);
    margin-top: var(--space-1);
  }

  .agent-node:last-child .node-line { display: none; }

  .node-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border);
  }

  .agent-node:last-child .node-body { border-bottom: none; }

  .node-name {
    font-weight: 600;
    font-size: var(--text-sm);
    color: var(--color-text);
  }

  .node-meta {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .node-id {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .json-hint { font-size: var(--text-xs); margin-top: var(--space-1); margin-bottom: 0; }
  .json-hint.valid { color: var(--color-success); }
  .json-hint.invalid { color: var(--color-danger); }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0,0,0,0); white-space: nowrap; border: 0; }

  @media (prefers-reduced-motion: reduce) {
    .spinner { animation: none; }
    .primary-btn,
    .secondary-btn,
    .danger-btn,
    .spec-editor,
    .id-input { transition: none; }
  }
</style>
