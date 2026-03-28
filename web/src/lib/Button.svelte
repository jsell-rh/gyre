<script>
  let {
    variant = 'primary',
    size = 'md',
    disabled = false,
    type = 'button',
    onclick = undefined,
    'aria-label': ariaLabel = undefined,
    'aria-describedby': ariaDescribedby = undefined,
    'aria-pressed': ariaPressed = undefined,
    'aria-expanded': ariaExpanded = undefined,
    'aria-busy': ariaBusy = undefined,
    'aria-controls': ariaControls = undefined,
    children,
  } = $props();
</script>

<button
  {type}
  {disabled}
  class="btn btn-{variant} btn-{size}"
  {onclick}
  aria-label={ariaLabel}
  aria-describedby={ariaDescribedby}
  aria-pressed={ariaPressed}
  aria-expanded={ariaExpanded}
  aria-busy={ariaBusy}
  aria-controls={ariaControls}
>
  {@render children?.()}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    font-family: var(--font-body);
    font-weight: 500;
    border: 1px solid transparent;
    border-radius: var(--radius);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast), color var(--transition-fast), transform 50ms ease;
    text-decoration: none;
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn:active:not(:disabled) {
    transform: scale(0.98);
  }

  /* Sizes */
  .btn-sm { padding: var(--space-1) var(--space-3); font-size: var(--text-xs); }
  .btn-md { padding: var(--space-2) var(--space-4); font-size: var(--text-sm); }
  .btn-lg { padding: var(--space-3) var(--space-5); font-size: var(--text-base); }

  /* Primary — Red Hat red */
  .btn-primary {
    background: var(--color-primary);
    color: var(--color-text-inverse);
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
    color: var(--color-text-inverse);
    border-color: var(--color-danger);
  }
  .btn-danger:hover:not(:disabled) {
    background: var(--color-danger-hover);
    border-color: var(--color-danger-hover);
  }

  .btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .btn { transition: none; transform: none; }
  }
</style>
