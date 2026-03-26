<script>
  let {
    label = '',
    helper = '',
    error = '',
    type = 'text',
    placeholder = '',
    value = $bindable(''),
    disabled = false,
    id = undefined,
  } = $props();

  let inputId = $derived(id ?? `input-${Math.random().toString(36).slice(2, 8)}`);
</script>

<div class="field" class:has-error={!!error}>
  {#if label}
    <label for={inputId} class="field-label">{label}</label>
  {/if}
  <input
    {type}
    {placeholder}
    {disabled}
    id={inputId}
    bind:value
    class="field-input"
    class:error={!!error}
    aria-describedby={error ? `${inputId}-error` : helper ? `${inputId}-helper` : undefined}
    aria-invalid={!!error}
  />
  {#if error}
    <span id="{inputId}-error" class="field-error">{error}</span>
  {:else if helper}
    <span id="{inputId}-helper" class="field-helper">{helper}</span>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .field-label {
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    color: var(--color-text);
  }

  .field-input {
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    outline: none;
    transition: border-color var(--transition-fast);
    width: 100%;
  }

  .field-input::placeholder {
    color: var(--color-text-muted);
  }

  .field-input:focus {
    border-color: var(--color-link);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-primary) 20%, transparent);
  }

  .field-input.error {
    border-color: var(--color-danger);
  }

  .field-input.error:focus {
    border-color: var(--color-danger);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--color-danger) 20%, transparent);
  }

  .field-input:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .field-helper {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  .field-error {
    font-size: var(--text-xs);
    color: var(--color-danger);
  }
</style>
