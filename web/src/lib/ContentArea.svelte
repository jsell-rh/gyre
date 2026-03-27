<script>
  import DetailPanel from './DetailPanel.svelte';

  /**
   * ContentArea — applies the right layout pattern based on `layout` prop and detailPanel state.
   *
   * Layouts:
   *   full-width     — single scrollable main area (default)
   *   split          — 60/40 main + detail panel (set automatically when detailPanel.open)
   *   canvas-controls — canvas fills top, controls below (used by Explorer at workspace/repo scope)
   *   editor-split   — 60/40 left editor + right preview (Meta-specs, Spec editing)
   */
  let {
    layout = 'full-width',
    detailPanel = null,
    onclosePanel = undefined,
    children,
  } = $props();

  // When detailPanel is open, override to split regardless of base layout
  let effectiveLayout = $derived(
    detailPanel?.open ? 'split' : layout
  );


</script>

<div
  class="content-area"
  class:full-width={effectiveLayout === 'full-width'}
  class:split={effectiveLayout === 'split'}
  class:canvas-controls={effectiveLayout === 'canvas-controls'}
  class:editor-split={effectiveLayout === 'editor-split'}
>
  <div class="content-main">
    {@render children?.()}
  </div>

  {#if detailPanel?.open}
    <aside class="detail-panel-container" aria-label="Detail panel">
      <DetailPanel entity={detailPanel.entity} onclose={onclosePanel} />
    </aside>
  {/if}
</div>

<style>
  .content-area {
    flex: 1;
    display: flex;
    overflow: hidden;
    min-height: 0;
  }

  /* full-width: single scrollable column */
  .content-area.full-width {
    flex-direction: column;
  }

  /* split: 60/40 main + detail panel */
  .content-area.split {
    flex-direction: row;
  }

  /* canvas-controls: children handle internal layout (Explorer renders canvas + control bar) */
  .content-area.canvas-controls {
    flex-direction: column;
  }

  /* editor-split: 60/40 left editor + right preview */
  .content-area.editor-split {
    flex-direction: row;
  }

  .content-main {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-width: 0;
    transition: opacity var(--transition-fast);
  }

  .content-main.faded {
    opacity: 0;
  }

  /* Split layout: main compresses to 60% */
  .split .content-main {
    flex: 0 0 60%;
    max-width: 60%;
    min-width: 480px;
  }

  /* Editor-split layout: left panel is 60% */
  .editor-split .content-main {
    flex: 0 0 60%;
    max-width: 60%;
  }

  /* Detail panel: 40% sliding in from right */
  .detail-panel-container {
    flex: 0 0 40%;
    max-width: 40%;
    overflow: hidden;
    animation: slideInRight var(--transition-fast) ease-out;
  }

  @keyframes slideInRight {
    from {
      transform: translateX(100%);
      opacity: 0;
    }
    to {
      transform: translateX(0);
      opacity: 1;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .detail-panel-container {
      animation: none;
    }

    .content-main {
      transition: none;
    }
  }
</style>
