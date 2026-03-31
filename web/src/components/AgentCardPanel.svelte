<script>
  import { t } from 'svelte-i18n';
  import { api } from '../lib/api.js';
  import Badge from '../lib/Badge.svelte';
  import Skeleton from '../lib/Skeleton.svelte';
  import { toastSuccess, toastError } from '../lib/toast.svelte.js';

  let { agentId } = $props();

  let card = $state(null);
  let cardLoading = $state(true);
  let editing = $state(false);
  let saving = $state(false);
  let saveError = $state(null);

  // Editable fields
  let capInput = $state('');
  let protocols = $state('');
  let endpoint = $state('');
  let description = $state('');

  // Load agent card whenever agentId changes
  $effect(() => {
    const id = agentId;
    if (!id) { card = null; cardLoading = false; return; }
    cardLoading = true;
    editing = false;
    api.agentCard(id)
      .then((data) => { card = data ?? null; })
      .catch(() => { card = null; })
      .finally(() => { cardLoading = false; });
  });

  function startEdit() {
    capInput = (card?.capabilities ?? []).join(', ');
    protocols = (card?.protocols ?? []).join(', ');
    endpoint = card?.endpoint ?? '';
    description = card?.description ?? '';
    editing = true;
  }

  async function saveCard() {
    saving = true;
    saveError = null;
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
      toastSuccess($t('agent_card.saved'));
    } catch (e) {
      saveError = e.message;
      toastError(e.message);
    } finally {
      saving = false;
    }
  }
</script>

<div class="card-panel">
  <div class="card-header">
    <div class="card-title-row">
      <span class="section-label">{$t('agent_card.title')}</span>
      <span class="protocol-tag">A2A</span>
    </div>
    {#if !editing && card}
      <button class="edit-btn" aria-label="Edit agent card" onclick={startEdit}>{$t('agent_card.edit')}</button>
    {/if}
  </div>

  {#if editing}
    <form class="edit-form" onsubmit={(e) => { e.preventDefault(); saveCard(); }}>
      <div class="field">
        <label class="field-label" for="desc-input">{$t('agent_card.description')}</label>
        <textarea
          id="desc-input"
          class="field-textarea"
          bind:value={description}
          placeholder={$t('agent_card.description_placeholder')}
          rows="3"
        ></textarea>
      </div>

      <div class="field">
        <label class="field-label" for="cap-input">{$t('agent_card.capabilities')}</label>
        <input
          id="cap-input"
          class="field-input"
          bind:value={capInput}
          placeholder={$t('agent_card.capabilities_placeholder')}
        />
        <span class="field-hint">{$t('agent_card.capabilities_hint')}</span>
      </div>

      <div class="field">
        <label class="field-label" for="proto-input">{$t('agent_card.protocols')}</label>
        <input
          id="proto-input"
          class="field-input"
          bind:value={protocols}
          placeholder={$t('agent_card.protocols_placeholder')}
        />
        <span class="field-hint">{$t('agent_card.capabilities_hint')}</span>
      </div>

      <div class="field">
        <label class="field-label" for="endpoint-input">{$t('agent_card.endpoint')}</label>
        <input
          id="endpoint-input"
          class="field-input"
          bind:value={endpoint}
          placeholder={$t('agent_card.endpoint_placeholder')}
        />
      </div>

      {#if saveError}
        <div class="form-error" role="alert">{saveError}</div>
      {/if}

      <div class="form-actions">
        <button type="button" class="cancel-btn" onclick={() => (editing = false)}>{$t('common.cancel')}</button>
        <button type="submit" class="save-btn" disabled={saving}>
          {saving ? $t('agent_card.saving') : $t('agent_card.save_card')}
        </button>
      </div>
    </form>

  {:else if card}
    <div class="card-body">
      {#if card.description}
        <p class="card-description">{card.description}</p>
      {/if}

      {#if card.capabilities?.length > 0}
        <div class="card-section">
          <span class="section-mini-label">{$t('agent_card.capabilities')}</span>
          <div class="pill-row">
            {#each card.capabilities as cap}
              <Badge value={cap} variant="info" />
            {/each}
          </div>
        </div>
      {/if}

      {#if card.protocols?.length > 0}
        <div class="card-section">
          <span class="section-mini-label">{$t('agent_card.protocols')}</span>
          <div class="pill-row">
            {#each card.protocols as proto}
              <Badge value={proto} variant="muted" />
            {/each}
          </div>
        </div>
      {/if}

      {#if card.endpoint}
        <div class="card-section">
          <span class="section-mini-label">{$t('agent_card.endpoint')}</span>
          <code class="endpoint-value">{card.endpoint}</code>
        </div>
      {/if}

      <button class="edit-btn-secondary" onclick={startEdit}>{$t('agent_card.edit_agent_card')}</button>
    </div>

  {:else if cardLoading}
    <div class="empty-card" aria-busy="true">
      <Skeleton width="60%" height="1rem" />
      <Skeleton lines={2} height="0.875rem" />
    </div>
  {:else}
    <div class="empty-card">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="24" height="24" aria-hidden="true">
        <rect x="2" y="5" width="20" height="14" rx="2"/>
        <path d="M2 10h20"/>
      </svg>
      <p class="empty-text">{$t('agent_card.no_card')}</p>
      <button class="publish-btn" onclick={startEdit}>{$t('agent_card.publish_card')}</button>
    </div>
  {/if}
</div>

<style>
  .card-panel {
    margin-top: var(--space-4);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .card-title-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .section-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .protocol-tag {
    font-size: var(--text-xs);
    font-weight: 600;
    background: color-mix(in srgb, var(--color-blocked) 15%, transparent);
    color: var(--color-blocked);
    border: 1px solid color-mix(in srgb, var(--color-blocked) 30%, transparent);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-2);
  }

  .edit-btn {
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-link);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-2);
    font-family: var(--font-body);
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }
  .edit-btn:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-link);
  }

  /* Edit form */
  .edit-form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-4);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .field-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .field-input, .field-textarea {
    background: var(--color-bg);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
    font-size: var(--text-sm);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast);
  }

  .field-textarea {
    resize: vertical;
    min-height: 80px;
    line-height: 1.5;
  }

  .field-input:focus:not(:focus-visible),
  .field-textarea:focus:not(:focus-visible) { outline: none; }
  .field-input:focus-visible,
  .field-textarea:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-color: var(--color-focus); }

  .field-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .form-error {
    color: var(--color-danger);
    font-size: var(--text-sm);
    background: color-mix(in srgb, var(--color-danger) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-danger) 20%, transparent);
    border-radius: var(--radius);
    padding: var(--space-2) var(--space-3);
  }

  .form-actions {
    display: flex;
    gap: var(--space-2);
    justify-content: flex-end;
  }

  .cancel-btn {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    font-family: var(--font-body);
    transition: border-color var(--transition-fast);
  }
  .cancel-btn:hover { border-color: var(--color-border-strong); }

  .save-btn {
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
  .save-btn:hover { background: var(--color-primary-hover); }
  .save-btn:disabled { opacity: 0.5; cursor: not-allowed; }

  /* Card body (view mode) */
  .card-body {
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .card-description {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.6;
  }

  .card-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .section-mini-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .pill-row {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .endpoint-value {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-link);
    background: color-mix(in srgb, var(--color-info) 8%, transparent);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    border: 1px solid color-mix(in srgb, var(--color-info) 20%, transparent);
  }

  .edit-btn-secondary {
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
    font-family: var(--font-body);
    align-self: flex-start;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }
  .edit-btn-secondary:hover {
    border-color: var(--color-border-strong);
    color: var(--color-text);
  }

  /* Empty state */
  .empty-card {
    padding: var(--space-6) var(--space-4);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    color: var(--color-text-muted);
  }

  .empty-text {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  .publish-btn {
    background: color-mix(in srgb, var(--color-info) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-info) 30%, transparent);
    border-radius: var(--radius);
    color: var(--color-link);
    cursor: pointer;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-4);
    font-family: var(--font-body);
    font-weight: 500;
    transition: background var(--transition-fast);
  }
  .publish-btn:hover { background: color-mix(in srgb, var(--color-info) 20%, transparent); }

  .edit-btn:focus-visible,
  .cancel-btn:focus-visible,
  .save-btn:focus-visible,
  .edit-btn-secondary:focus-visible,
  .publish-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .protocol-tag,
    .edit-btn,
    .field-input,
    .field-textarea,
    .cancel-btn,
    .save-btn,
    .edit-btn-secondary,
    .publish-btn {
      transition: none;
    }
  }
</style>
