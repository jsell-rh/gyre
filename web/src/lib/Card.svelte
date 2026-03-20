<script>
  let { class: cls = '', header = undefined, footer = undefined, children } = $props();
</script>

<div class="card {cls}">
  {#if header}
    <div class="card-header">{@render header()}</div>
  {/if}
  <div class="card-body">
    {@render children?.()}
  </div>
  {#if footer}
    <div class="card-footer">{@render footer()}</div>
  {/if}
</div>

<style>
  .card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .card-header {
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    font-family: var(--font-display);
    font-weight: 600;
    font-size: var(--text-base);
    color: var(--color-text);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }

  .card-body {
    padding: var(--space-6);
  }

  .card-footer {
    padding: var(--space-4) var(--space-6);
    border-top: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
</style>
