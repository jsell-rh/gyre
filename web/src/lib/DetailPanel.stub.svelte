<!-- web/src/lib/DetailPanel.stub.svelte — TEMPORARY stub (replaced by S4.8) -->
<script>
  let { entity, onclose } = $props();
</script>

<div class="detail-panel-stub">
  <div class="stub-header">
    <span class="stub-title">Detail Panel (S4.8 pending)</span>
    <button class="stub-close" onclick={onclose} aria-label="Close detail panel">✕</button>
  </div>
  {#if entity}
    <div class="stub-body" style="color: var(--color-text-muted); font-size: var(--text-sm)">
      <div><strong>Type:</strong> {entity.type}</div>
      <div><strong>ID:</strong> {entity.id}</div>
    </div>
  {/if}
</div>

<style>
  .detail-panel-stub {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
  }

  .stub-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
  }

  .stub-title {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .stub-close {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-base);
    padding: 0;
    line-height: 1;
  }

  .stub-close:hover {
    color: var(--color-text);
  }

  .stub-body {
    padding: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
</style>
