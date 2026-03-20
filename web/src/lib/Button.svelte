<script>
  let {
    variant = 'primary',
    size = 'md',
    disabled = false,
    type = 'button',
    onclick = undefined,
    children,
  } = $props();
</script>

<button
  {type}
  {disabled}
  class="btn btn-{variant} btn-{size}"
  {onclick}
>
  {@render children?.()}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.375rem;
    font-family: var(--font-body);
    font-weight: 500;
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast), color var(--transition-fast);
    text-decoration: none;
  }

  .btn:disabled {
    opacity: 0.45;
    cursor: not-allowed;
    pointer-events: none;
  }

  /* Sizes */
  .btn-sm { padding: 0.25rem 0.625rem; font-size: var(--text-xs); }
  .btn-md { padding: 0.4375rem 0.875rem; font-size: var(--text-sm); }
  .btn-lg { padding: 0.625rem 1.25rem; font-size: var(--text-base); }

  /* Primary — Red Hat red */
  .btn-primary {
    background: var(--color-primary);
    color: #fff;
    border-color: var(--color-primary);
  }
  .btn-primary:hover:not(:disabled) {
    background: var(--color-primary-hover);
    border-color: var(--color-primary-hover);
  }

  /* Secondary — gray surface */
  .btn-secondary {
    background: var(--color-surface-elevated);
    color: var(--color-text);
    border-color: var(--color-border-strong);
  }
  .btn-secondary:hover:not(:disabled) {
    background: var(--color-border);
    border-color: var(--color-text-muted);
  }

  /* Ghost — transparent */
  .btn-ghost {
    background: transparent;
    color: var(--color-text-secondary);
    border-color: transparent;
  }
  .btn-ghost:hover:not(:disabled) {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  /* Danger */
  .btn-danger {
    background: var(--color-danger);
    color: #fff;
    border-color: var(--color-danger);
  }
  .btn-danger:hover:not(:disabled) {
    background: #c43a10;
    border-color: #c43a10;
  }
</style>
