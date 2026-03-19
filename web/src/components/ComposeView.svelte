<script>
  import { api } from '../lib/api.js';

  let composeId = $state('');
  let statusAgents = $state([]);
  let statusLoading = $state(false);
  let statusError = $state(null);

  let yamlInput = $state('');
  let applyLoading = $state(false);
  let applyError = $state(null);
  let applyResult = $state(null);

  const placeholderText = '{"version":"1","project_id":"...","repo_id":"...","agents":[...]}';

  let teardownLoading = $state(false);
  let teardownError = $state(null);

  async function applyCompose() {
    if (!yamlInput.trim()) { applyError = 'Compose spec is required.'; return; }
    applyLoading = true; applyError = null; applyResult = null;
    try {
      const spec = JSON.parse(yamlInput);
      applyResult = await api.composeApply(spec);
      composeId = applyResult.compose_id;
      await loadStatus();
    } catch (e) {
      applyError = e.message;
    } finally {
      applyLoading = false;
    }
  }

  async function loadStatus() {
    if (!composeId.trim()) return;
    statusLoading = true; statusError = null;
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
    teardownLoading = true; teardownError = null;
    try {
      await api.composeTeardown(composeId);
      statusAgents = [];
      applyResult = null;
      composeId = '';
    } catch (e) {
      teardownError = e.message;
    } finally {
      teardownLoading = false;
    }
  }

  const statusColors = {
    idle: '#94a3b8',
    active: '#4ade80',
    blocked: '#f97316',
    error: '#f87171',
    dead: '#64748b',
  };
</script>

<div class="panel">
  <div class="panel-header">
    <h2>Agent Compose</h2>
  </div>

  <div class="scroll">
    <!-- Apply section -->
    <section class="section">
      <h3>Apply Compose Spec</h3>
      <p class="hint">Paste a JSON agent-compose spec. YAML not supported in browser — use JSON.</p>
      <textarea
        class="yaml-editor"
        bind:value={yamlInput}
        placeholder={placeholderText}
        rows="8"
      ></textarea>
      {#if applyError}
        <p class="form-error">{applyError}</p>
      {/if}
      <button class="btn primary" onclick={applyCompose} disabled={applyLoading}>
        {applyLoading ? 'Applying…' : 'Apply Compose'}
      </button>

      {#if applyResult}
        <div class="apply-result">
          <p class="success-msg">Compose applied. Session ID: <code>{applyResult.compose_id}</code></p>
        </div>
      {/if}
    </section>

    <!-- Status section -->
    <section class="section">
      <h3>Compose Status</h3>
      <div class="status-controls">
        <input
          class="id-input"
          bind:value={composeId}
          placeholder="Compose session ID"
        />
        <button class="btn" onclick={loadStatus} disabled={statusLoading}>
          {statusLoading ? 'Loading…' : 'Refresh'}
        </button>
        <button class="btn danger" onclick={doTeardown} disabled={teardownLoading || !composeId}>
          {teardownLoading ? 'Stopping…' : 'Teardown'}
        </button>
      </div>
      {#if teardownError}
        <p class="form-error">{teardownError}</p>
      {/if}
      {#if statusError}
        <p class="form-error">{statusError}</p>
      {:else if statusAgents.length === 0 && !statusLoading}
        <p class="state-msg muted">No agents in this compose session.</p>
      {:else}
        <ul class="agent-tree">
          {#each statusAgents as agent (agent.agent_id)}
            <li class="agent-node">
              <span class="agent-name">{agent.name}</span>
              <span
                class="agent-status"
                style:color={statusColors[agent.status] ?? 'var(--text-muted)'}
              >{agent.status}</span>
              <span class="agent-id">{agent.agent_id.slice(0, 8)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  </div>
</div>

<style>
  .panel { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .panel-header {
    display: flex; align-items: center; padding: 1rem 1.25rem;
    border-bottom: 1px solid var(--border); flex-shrink: 0;
  }

  h2 { margin: 0; font-size: 1rem; font-weight: 600; color: var(--text); }
  h3 { margin: 0 0 0.5rem; font-size: 0.88rem; font-weight: 600; color: var(--text); }

  .scroll { flex: 1; overflow-y: auto; padding: 0.75rem 1.25rem; display: flex; flex-direction: column; gap: 1.5rem; }

  .section {
    display: flex; flex-direction: column; gap: 0.6rem;
    padding: 1rem; background: var(--surface); border: 1px solid var(--border-subtle); border-radius: 6px;
  }

  .hint { font-size: 0.78rem; color: var(--text-dim); margin: 0; }

  .yaml-editor {
    width: 100%; background: var(--bg); border: 1px solid var(--border); border-radius: 4px;
    color: var(--text); font-family: 'Courier New', monospace; font-size: 0.78rem;
    padding: 0.5rem; resize: vertical; min-height: 120px;
  }

  .form-error { color: #f87171; font-size: 0.82rem; margin: 0; }
  .success-msg { color: #4ade80; font-size: 0.82rem; margin: 0; }

  .status-controls { display: flex; gap: 0.5rem; align-items: center; }

  .id-input {
    flex: 1; background: var(--bg); border: 1px solid var(--border); border-radius: 4px;
    color: var(--text); font-size: 0.82rem; padding: 0.35rem 0.6rem;
  }

  .btn {
    background: var(--surface); border: 1px solid var(--border); border-radius: 4px;
    color: var(--text); font-size: 0.82rem; padding: 0.35rem 0.9rem; cursor: pointer;
    white-space: nowrap;
  }
  .btn:hover { background: var(--surface-hover); }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn.primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn.primary:hover { opacity: 0.88; }
  .btn.danger { background: #991b1b18; border-color: #991b1b; color: #f87171; }
  .btn.danger:hover { background: #991b1b30; }

  .apply-result { padding: 0.6rem; background: var(--bg); border-radius: 4px; border: 1px solid var(--border-subtle); }

  .agent-tree { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 0.3rem; }

  .agent-node {
    display: flex; align-items: center; gap: 0.75rem;
    padding: 0.5rem 0.75rem; background: var(--bg);
    border: 1px solid var(--border-subtle); border-radius: 4px; font-size: 0.85rem;
  }

  .agent-name { font-weight: 600; color: var(--text); flex: 1; }
  .agent-status { font-size: 0.78rem; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; }
  .agent-id { font-family: monospace; font-size: 0.72rem; color: var(--text-dim); }

  .state-msg { padding: 1rem; color: var(--text-dim); text-align: center; font-style: italic; font-size: 0.85rem; }
</style>
