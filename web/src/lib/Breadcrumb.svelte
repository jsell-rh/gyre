<script>
  let { items = [], onnavigate = undefined } = $props();
  // items: Array<{ label: string, view?: string, ctx?: object }>
</script>

{#if items.length > 0}
  <nav class="breadcrumb" aria-label="Breadcrumb">
    <ol>
      {#each items as item, i}
        <li>
          {#if i < items.length - 1 && item.view}
            <button
              class="crumb-link"
              onclick={() => onnavigate?.(item.view, item.ctx)}
            >{item.label}</button>
            <span class="crumb-sep" aria-hidden="true">/</span>
          {:else}
            <span class="crumb-current" aria-current="page">{item.label}</span>
          {/if}
        </li>
      {/each}
    </ol>
  </nav>
{/if}

<style>
  .breadcrumb {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  ol {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    list-style: none;
    flex-wrap: wrap;
  }

  li {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .crumb-link {
    background: transparent;
    border: none;
    color: var(--color-text-secondary);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-body);
    padding: 0;
    transition: color var(--transition-fast);
  }

  .crumb-link:hover {
    color: var(--color-link-hover);
  }

  .crumb-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }

  .crumb-sep {
    color: var(--color-text-muted);
    user-select: none;
  }

  .crumb-current {
    color: var(--color-text-secondary);
    font-weight: 500;
  }

  @media (prefers-reduced-motion: reduce) {
    .crumb-link { transition: none; }
  }
</style>
