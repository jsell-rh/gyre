<script>
  let {
    title = 'Nothing here yet',
    description = '',
    icon = undefined,
    action = undefined,
    children,
  } = $props();
</script>

<div class="empty-state">
  <div class="empty-icon" aria-hidden="true">
    {#if icon}
      {@html icon}
    {:else}
      <svg viewBox="0 0 48 48" fill="none" width="48" height="48">
        <rect x="8" y="12" width="32" height="28" rx="3" stroke="currentColor" stroke-width="2"/>
        <path d="M16 12V10a2 2 0 012-2h12a2 2 0 012 2v2" stroke="currentColor" stroke-width="2"/>
        <path d="M20 24h8M20 30h5" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>
      </svg>
    {/if}
  </div>
  <p class="empty-title">{title}</p>
  {#if description}
    <p class="empty-description">{description}</p>
  {/if}
  {#if children}
    <div class="empty-action">
      {@render children()}
    </div>
  {:else if action}
    <div class="empty-action">{@render action()}</div>
  {/if}
</div>

<style>
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: var(--space-12) var(--space-8);
    gap: var(--space-3);
  }

  .empty-icon {
    color: var(--color-text-muted);
    margin-bottom: var(--space-2);
    opacity: 0.6;
  }

  .empty-title {
    font-family: var(--font-display);
    font-size: var(--text-lg);
    font-weight: 600;
    color: var(--color-text);
  }

  .empty-description {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    max-width: 360px;
    line-height: 1.6;
  }

  .empty-action {
    margin-top: var(--space-2);
  }
</style>
