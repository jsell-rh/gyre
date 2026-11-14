<script>
  /**
   * PolicyEditor — ABAC policy create/edit form (abac-policy-engine.md §UI
   * "Policy editor") with a visual condition builder whose attribute field
   * autocompletes from the spec's §Attributes catalog (Subject / Resource /
   * Environment).
   *
   * Create: POST /api/v1/policies — Update: PUT /api/v1/policies/{id}
   * (global endpoints; scope is a form field, never a URL prefix).
   */
  import Modal from '../../lib/Modal.svelte';
  import { api } from '../../lib/api.js';
  import {
    ALL_ATTRIBUTES,
    CONDITION_OPERATORS,
    KNOWN_ACTIONS,
    KNOWN_RESOURCE_TYPES,
    attributeSuggestions,
  } from '../../lib/abac.js';

  let {
    open = $bindable(false),
    policy = null,           // null → create; policy object → edit
    defaultScope = 'tenant',
    defaultScopeId = '',
    lockScope = false,       // embedding settings page fixes the scope
    onsaved = undefined,
  } = $props();

  const datalistId = 'abac-attribute-list-' + Math.random().toString(36).slice(2);
  const actionsDatalistId = 'abac-action-list-' + Math.random().toString(36).slice(2);
  const resourceTypesDatalistId = 'abac-resource-type-list-' + Math.random().toString(36).slice(2);

  let name = $state('');
  let description = $state('');
  let scope = $state(defaultScope);
  let scopeId = $state(defaultScopeId ?? '');
  let priority = $state(100);
  let effect = $state('allow');
  let actionsText = $state('*');
  let resourceTypesText = $state('*');
  let enabled = $state(true);
  let conditions = $state([]);
  let saving = $state(false);
  let saveError = $state(null);

  // Reset the form each time the editor opens (create vs edit shape).
  $effect(() => {
    if (open) {
      const p = policy;
      name = p?.name ?? '';
      description = p?.description ?? '';
      scope = (p?.scope ?? defaultScope ?? 'tenant').toLowerCase();
      scopeId = p?.scope_id ?? defaultScopeId ?? '';
      priority = p?.priority ?? 100;
      effect = (p?.effect ?? 'allow').toLowerCase();
      actionsText = Array.isArray(p?.actions) && p.actions.length ? p.actions.join(', ') : '*';
      resourceTypesText = Array.isArray(p?.resource_types) && p.resource_types.length ? p.resource_types.join(', ') : '*';
      enabled = p?.enabled !== false;
      conditions = (p?.conditions ?? []).map((c) => ({
        attribute: c?.attribute ?? '',
        operator: (c?.operator ?? 'equals').toLowerCase(),
        // Value side: arrays render comma-joined so `in` round-trips.
        value: Array.isArray(c?.value) ? c.value.join(', ')
          : c?.value == null ? '' : String(c.value),
      }));
      saveError = null;
    }
  });

  let isEdit = $derived(policy != null && policy.id != null);

  function addCondition() {
    conditions = [...conditions, { attribute: '', operator: 'equals', value: '' }];
  }

  function removeCondition(index) {
    conditions = conditions.filter((_, i) => i !== index);
  }

  function splitList(text) {
    return String(text ?? '')
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean);
  }

  /**
   * Serialize one condition row to the server's ConditionRequest shape
   * ({attribute, operator, value}). Value typing mirrors parse_condition:
   * `in`/`not_in` → string array, numeric comparisons → number when the
   * text is numeric, `exists` → null, otherwise a string.
   */
  function serializeCondition(c) {
    let value;
    if (c.operator === 'exists') {
      value = null;
    } else if (c.operator === 'in' || c.operator === 'not_in') {
      value = splitList(c.value);
    } else if (c.operator === 'greater_than' || c.operator === 'less_than') {
      const num = Number(c.value);
      value = c.value !== '' && !Number.isNaN(num) ? num : String(c.value);
    } else {
      value = String(c.value);
    }
    return { attribute: c.attribute.trim(), operator: c.operator, value };
  }

  function buildPayload() {
    const payload = {
      name: name.trim(),
      description: description.trim(),
      scope,
      priority: parseInt(priority, 10) || 0,
      effect,
      conditions: conditions
        .filter((c) => c.attribute.trim() !== '')
        .map(serializeCondition),
      actions: splitList(actionsText).length ? splitList(actionsText) : ['*'],
      resource_types: splitList(resourceTypesText).length ? splitList(resourceTypesText) : ['*'],
      enabled,
    };
    if (scopeId.trim()) payload.scope_id = scopeId.trim();
    return payload;
  }

  let canSave = $derived(name.trim() !== '' && !saving);

  async function save() {
    if (!canSave) return;
    saving = true;
    saveError = null;
    try {
      if (isEdit) {
        await api.updatePolicy(policy.id, buildPayload());
      } else {
        await api.createPolicy(buildPayload());
      }
      open = false;
      onsaved?.();
    } catch (e) {
      saveError = e?.message ?? String(e);
    } finally {
      saving = false;
    }
  }
</script>

<Modal {open} title={isEdit ? 'Edit Policy' : 'Create Policy'} size="lg" onclose={() => (open = false)} onsubmit={save}>
  <div class="policy-editor" data-testid="policy-editor">
    {#if saveError}
      <p class="error-text" role="alert" data-testid="policy-editor-error">{saveError}</p>
    {/if}

    <div class="form-grid">
      <label class="field">
        <span class="field-label">Name</span>
        <input class="field-input" bind:value={name} placeholder="e.g. allow-agent-push" data-testid="policy-name-input" />
      </label>
      <label class="field">
        <span class="field-label">Effect</span>
        <select class="field-input" bind:value={effect} data-testid="policy-effect-select">
          <option value="allow">Allow</option>
          <option value="deny">Deny</option>
        </select>
      </label>
      <label class="field">
        <span class="field-label">Priority</span>
        <input class="field-input" type="number" bind:value={priority} data-testid="policy-priority-input" />
      </label>
      <label class="field">
        <span class="field-label">Scope</span>
        <select class="field-input" bind:value={scope} disabled={lockScope} data-testid="policy-scope-select">
          <option value="tenant">Tenant</option>
          <option value="workspace">Workspace</option>
          <option value="repo">Repo</option>
        </select>
      </label>
      <label class="field">
        <span class="field-label">Scope ID <span class="field-hint">(blank = all at this scope)</span></span>
        <input
          class="field-input"
          bind:value={scopeId}
          disabled={lockScope}
          placeholder="tenant / workspace / repo id"
          data-testid="policy-scope-id-input"
        />
      </label>
    </div>

    <label class="field">
      <span class="field-label">Description</span>
      <input class="field-input" bind:value={description} placeholder="What this policy does" data-testid="policy-description-input" />
    </label>

    <div class="form-grid">
      <label class="field">
        <span class="field-label">Actions <span class="field-hint">(comma-separated)</span></span>
        <input class="field-input" bind:value={actionsText} list={actionsDatalistId} placeholder="push, read" data-testid="policy-actions-input" />
      </label>
      <label class="field">
        <span class="field-label">Resource types <span class="field-hint">(comma-separated)</span></span>
        <input class="field-input" bind:value={resourceTypesText} list={resourceTypesDatalistId} placeholder="repo, agent" data-testid="policy-resource-types-input" />
      </label>
    </div>

    <label class="toggle-row">
      <input type="checkbox" bind:checked={enabled} data-testid="policy-enabled-toggle" />
      <span>Enabled</span>
    </label>

    <!-- Visual condition builder (§UI): attribute autocomplete from the
         §Attributes catalog via datalist; operator + value per row. -->
    <div class="conditions-section">
      <div class="conditions-header">
        <span class="field-label">Conditions <span class="field-hint">(all must match — AND)</span></span>
        <button type="button" class="btn-small" onclick={addCondition} data-testid="add-condition-btn">+ Add condition</button>
      </div>

      <datalist id={datalistId}>
        {#each ALL_ATTRIBUTES as attr (attr.name)}
          <option value={attr.name}>{attr.category} — {attr.description}</option>
        {/each}
      </datalist>
      <datalist id={actionsDatalistId}>
        {#each KNOWN_ACTIONS as action (action)}
          <option value={action}></option>
        {/each}
      </datalist>
      <datalist id={resourceTypesDatalistId}>
        {#each KNOWN_RESOURCE_TYPES as rt (rt)}
          <option value={rt}></option>
        {/each}
      </datalist>

      {#if conditions.length === 0}
        <p class="hint-text">No conditions — this policy matches every request in its action / resource / scope range.</p>
      {:else}
        <div class="condition-rows" data-testid="condition-rows">
          {#each conditions as condition, index (index)}
            <div class="condition-row" data-testid="condition-row">
              <span class="attr-cell">
                <input
                  class="field-input condition-attr"
                  bind:value={condition.attribute}
                  list={datalistId}
                  placeholder="subject.workspace_role"
                  data-testid="condition-attribute-input"
                  aria-label="Condition {index + 1} attribute"
                />
                {#if condition.attribute && attributeSuggestions(condition.attribute).length === 1 && attributeSuggestions(condition.attribute)[0].name === condition.attribute}
                  <span class="attr-hint" title={attributeSuggestions(condition.attribute)[0].description}>
                    e.g. {attributeSuggestions(condition.attribute)[0].examples}
                  </span>
                {/if}
              </span>
              <select
                class="field-input condition-op"
                bind:value={condition.operator}
                aria-label="Condition {index + 1} operator"
                data-testid="condition-operator-select"
              >
                {#each CONDITION_OPERATORS as op (op)}
                  <option value={op}>{op}</option>
                {/each}
              </select>
              {#if condition.operator !== 'exists'}
                <input
                  class="field-input condition-val"
                  bind:value={condition.value}
                  placeholder={condition.operator === 'in' || condition.operator === 'not_in' ? 'a, b, c' : 'value'}
                  aria-label="Condition {index + 1} value"
                  data-testid="condition-value-input"
                />
              {/if}
              <button
                type="button"
                class="btn-small btn-danger"
                onclick={() => removeCondition(index)}
                aria-label="Remove condition {index + 1}"
                data-testid="remove-condition-btn"
              >
                ✕
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>

  {#snippet footer()}
    <button type="button" class="btn-small" onclick={() => (open = false)} data-testid="policy-editor-cancel">Cancel</button>
    <button type="button" class="btn-small btn-primary" onclick={save} disabled={!canSave} data-testid="policy-editor-save">
      {saving ? 'Saving…' : isEdit ? 'Save Changes' : 'Create Policy'}
    </button>
  {/snippet}
</Modal>

<style>
  .policy-editor {
    display: flex;
    flex-direction: column;
    gap: var(--space-4, 16px);
  }

  .form-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-3, 12px);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1, 4px);
  }

  .field-label {
    font-size: var(--text-sm, 13px);
    font-weight: 600;
    color: var(--color-text, inherit);
  }

  .field-hint {
    font-weight: 400;
    color: var(--color-text-muted, #888);
  }

  .field-input {
    padding: 6px 10px;
    border: 1px solid var(--color-border, #ccc);
    border-radius: var(--radius, 6px);
    background: var(--color-surface, #fff);
    color: inherit;
    font: inherit;
    min-width: 0;
  }

  .field-input:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .toggle-row {
    display: flex;
    align-items: center;
    gap: var(--space-2, 8px);
  }

  .conditions-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-2, 8px);
    padding-top: var(--space-2, 8px);
    border-top: 1px solid var(--color-border, #eee);
  }

  .conditions-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2, 8px);
  }

  .condition-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2, 8px);
  }

  .condition-row {
    display: grid;
    grid-template-columns: minmax(160px, 1.4fr) minmax(110px, auto) minmax(120px, 1fr) auto;
    align-items: center;
    gap: var(--space-2, 8px);
  }

  .attr-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .attr-hint {
    font-size: var(--text-xs, 11px);
    color: var(--color-text-muted, #888);
  }

  .hint-text {
    color: var(--color-text-muted, #888);
    font-size: var(--text-sm, 13px);
  }

  .error-text {
    color: var(--color-danger, #c0392b);
    margin: 0;
  }

  .btn-small {
    padding: 5px 12px;
    border: 1px solid var(--color-border, #ccc);
    border-radius: var(--radius, 6px);
    background: var(--color-surface-elevated, #f5f5f5);
    color: inherit;
    cursor: pointer;
    font: inherit;
    font-size: var(--text-sm, 13px);
  }

  .btn-small:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-primary {
    background: var(--color-primary, #2563eb);
    border-color: var(--color-primary, #2563eb);
    color: #fff;
  }

  .btn-danger {
    color: var(--color-danger, #c0392b);
  }
</style>
