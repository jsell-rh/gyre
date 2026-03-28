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
        <circle cx="24" cy="24" r="18" stroke="currentColor" stroke-width="1.5" stroke-dasharray="4 3"/>
        <path d="M18 24h12M24 18v12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" opacity="0.5"/>
      </svg>
    {/if}
  </div>
  {#if title}
    <p class="empty-title">{title}</p>
  {/if}
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
    opacity: 0.75;
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
