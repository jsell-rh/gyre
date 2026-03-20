<script>
  let {
    width = '100%',
    height = '1rem',
    radius = 'var(--radius)',
    lines = 1,
  } = $props();
</script>

{#if lines > 1}
  <div class="skeleton-lines">
    {#each Array(lines) as _, i}
      <div
        class="skeleton"
        style="width:{i === lines - 1 ? '70%' : '100%'}; height:{height}; border-radius:{radius}"
      ></div>
    {/each}
  </div>
{:else}
  <div
    class="skeleton"
    style="width:{width}; height:{height}; border-radius:{radius}"
  ></div>
{/if}

<style>
  .skeleton {
    background: linear-gradient(
      90deg,
      var(--color-surface-elevated) 25%,
      var(--color-border) 50%,
      var(--color-surface-elevated) 75%
    );
    background-size: 200% 100%;
    animation: shimmer 1.5s infinite;
    flex-shrink: 0;
  }

  .skeleton-lines {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  @keyframes shimmer {
    from { background-position: 200% 0; }
    to   { background-position: -200% 0; }
  }
</style>
