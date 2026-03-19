<script>
  import { api } from '../lib/api.js';

  let { agentId } = $props();

  let card = $state(null);
  let editing = $state(false);
  let saving = $state(false);
  let saveError = $state(null);

  // Editable fields
  let capInput = $state('');
  let protocols = $state('');
  let endpoint = $state('');
  let description = $state('');

  function startEdit() {
    capInput = (card?.capabilities ?? []).join(', ');
    protocols = (card?.protocols ?? []).join(', ');
    endpoint = card?.endpoint ?? '';
    description = card?.description ?? '';
    editing = true;
  }

  async function saveCard() {
    saving = true; saveError = null;
    try {
      const newCard = {
        agent_id: agentId,
        name: card?.name ?? agentId,
        description,
        capabilities: capInput.split(',').map((s) => s.trim()).filter(Boolean),
        protocols: protocols.split(',').map((s) => s.trim()).filter(Boolean),
        endpoint,
      };
      await api.updateAgentCard(agentId, newCard);
      card = newCard;
      editing = false;
    } catch (e) {
      saveError = e.message;
    } finally {
      saving = false;
    }
  }
</script>

<div class="card-panel">
  <div class="card-header">
    <span class="card-title">Agent Card (A2A)</span>
    {#if !editing}
      <button class="edit-btn" onclick={startEdit}>Edit</button>
    {/if}
  </div>

  {#if editing}
    <div class="form">
      <label>
        Description
        <input bind:value={description} placeholder="What this agent does" />
      </label>
      <label>
        Capabilities (comma-separated)
        <input bind:value={capInput} placeholder="rust, api-design, planning" />
      </label>
      <label>
        Protocols (comma-separated)
        <input bind:value={protocols} placeholder="mcp, a2a" />
      </label>
      <label>
        Endpoint URL
        <input bind:value={endpoint} placeholder="http://agent:3000" />
      </label>
      {#if saveError}
        <p class="form-error">{saveError}</p>
      {/if}
      <div class="form-actions">
        <button class="btn" onclick={() => (editing = false)}>Cancel</button>
        <button class="btn primary" onclick={saveCard} disabled={saving}>
          {saving ? 'Saving…' : 'Save Card'}
        </button>
      </div>
    </div>
  {:else if card}
    <dl class="card-dl">
      <dt>Description</dt><dd>{card.description || '—'}</dd>
      <dt>Capabilities</dt><dd>{card.capabilities?.join(', ') || '—'}</dd>
      <dt>Protocols</dt><dd>{card.protocols?.join(', ') || '—'}</dd>
      <dt>Endpoint</dt><dd class="mono">{card.endpoint || '—'}</dd>
    </dl>
  {:else}
    <p class="empty-card">No agent card published. <button class="link-btn" onclick={startEdit}>Publish one.</button></p>
  {/if}
</div>

<style>
  .card-panel {
    margin-top: 1rem; padding: 0.75rem; background: var(--bg);
    border: 1px solid var(--border-subtle); border-radius: 5px;
  }

  .card-header {
    display: flex; align-items: center; justify-content: space-between;
    margin-bottom: 0.5rem;
  }

  .card-title { font-size: 0.78rem; font-weight: 600; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.04em; }

  .edit-btn {
    background: none; border: 1px solid var(--border); border-radius: 4px;
    color: var(--accent); font-size: 0.75rem; padding: 0.15rem 0.5rem; cursor: pointer;
  }
  .edit-btn:hover { background: var(--surface-hover); }

  .card-dl { display: grid; grid-template-columns: 7rem 1fr; gap: 0.3rem 0.5rem; font-size: 0.82rem; }
  dt { color: var(--text-dim); }
  dd { margin: 0; color: var(--text-muted); }
  .mono { font-family: monospace; font-size: 0.78rem; }

  .form { display: flex; flex-direction: column; gap: 0.5rem; }

  .form label {
    display: flex; flex-direction: column; gap: 0.2rem;
    font-size: 0.78rem; color: var(--text-dim);
  }

  .form input {
    background: var(--surface); color: var(--text); border: 1px solid var(--border);
    border-radius: 4px; padding: 0.35rem 0.5rem; font-size: 0.82rem;
  }

  .form-error { color: #f87171; font-size: 0.78rem; margin: 0; }

  .form-actions { display: flex; gap: 0.4rem; justify-content: flex-end; }

  .btn {
    background: var(--surface); border: 1px solid var(--border); border-radius: 4px;
    color: var(--text); font-size: 0.78rem; padding: 0.3rem 0.7rem; cursor: pointer;
  }
  .btn.primary { background: var(--accent); color: #fff; border-color: var(--accent); }
  .btn:disabled { opacity: 0.5; cursor: not-allowed; }

  .empty-card { font-size: 0.82rem; color: var(--text-dim); margin: 0; font-style: italic; }
  .link-btn {
    background: none; border: none; color: var(--accent); cursor: pointer;
    font-size: inherit; padding: 0; text-decoration: underline;
  }
</style>
