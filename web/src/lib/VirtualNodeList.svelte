<script>
  /**
   * VirtualNodeList — windowed (virtual-scrolling) list of graph nodes.
   *
   * Backing store for the 'list' layout at high node counts
   * (ui-layout.md §10): when a view exceeds ~1000 nodes even after the
   * public-only auto-filter, the Explorer switches to this list, which
   * renders only the rows intersecting the scroll viewport (plus overscan)
   * instead of one DOM row per node.
   *
   * Rows are interactive elements: they emit standardized ViewEvents
   * (click / dblclick / hover / context-menu) and call `onSelectNode` on
   * click, matching the graph canvas contract.
   */
  import { dispatchViewEvent, viewEventFromDom } from './viewEvents.js';
  import Badge from './Badge.svelte';

  let {
    nodes = [],
    onSelectNode = undefined,
    rowHeight = 34,
    viewportHeight = 480,
    overscan = 6,
  } = $props();

  let containerEl = $state(null);
  let scrollTop = $state(0);

  // jsdom reports clientHeight 0 (no layout) — fall back to the prop so the
  // component still windows sensibly in tests that drive scrollTop directly.
  let measuredHeight = $derived(containerEl?.clientHeight || viewportHeight);

  let total = $derived(nodes.length);
  let totalHeight = $derived(total * rowHeight);
  let startIndex = $derived(Math.max(0, Math.floor(scrollTop / rowHeight) - overscan));
  let endIndex = $derived(
    Math.min(total, Math.ceil((scrollTop + measuredHeight) / rowHeight) + overscan)
  );
  let visibleNodes = $derived(nodes.slice(startIndex, endIndex));

  function onScroll(e) {
    scrollTop = e.currentTarget?.scrollTop ?? 0;
  }

  function emit(domEvent, node) {
    dispatchViewEvent(
      domEvent.currentTarget ?? null,
      viewEventFromDom(domEvent, 'node', node.id, node)
    );
  }

  function rowClick(domEvent, node) {
    emit(domEvent, node);
    onSelectNode?.(node);
  }

  function rowKeydown(e, node) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      rowClick(e, node);
    }
  }
</script>

<div
  class="vlist"
  bind:this={containerEl}
  style="height: {viewportHeight}px"
  onscroll={onScroll}
  role="listbox"
  aria-label="Graph nodes (virtualized)"
  tabindex="0"
>
  <div class="vlist-spacer" style="height: {totalHeight}px" aria-hidden="true"></div>
  <div class="vlist-window" style="transform: translateY({startIndex * rowHeight}px)">
    {#each visibleNodes as node (node.id)}
      <div
        class="vlist-row"
        role="option"
        aria-selected={false}
        tabindex="0"
        style="height: {rowHeight}px"
        onclick={(e) => rowClick(e, node)}
        ondblclick={(e) => emit(e, node)}
        onmouseenter={(e) => emit(e, node)}
        oncontextmenu={(e) => { e.preventDefault(); emit(e, node); }}
        onkeydown={(e) => rowKeydown(e, node)}
      >
        <span class="vlist-type"><Badge value={node.node_type ?? '?'} /></span>
        <span class="vlist-name" title={node.qualified_name ?? node.name}>{node.name}</span>
        <span class="vlist-file">{node.file_path ?? ''}</span>
      </div>
    {/each}
  </div>
  <span class="sr-only" aria-live="polite">{total} nodes</span>
</div>

<style>
  .vlist {
    position: relative;
    overflow-y: auto;
    border: 1px solid var(--color-border, #334155);
    border-radius: 8px;
    background: var(--color-bg, #0f172a);
  }
  .vlist-spacer {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    pointer-events: none;
  }
  .vlist-window {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    display: flex;
    flex-direction: column;
  }
  .vlist-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 12px;
    box-sizing: border-box;
    cursor: pointer;
    border-bottom: 1px solid var(--color-border, #1e293b);
    transition: background-color 120ms ease;
  }
  .vlist-row:hover,
  .vlist-row:focus-visible {
    background: var(--color-bg-hover, #1e293b);
    outline: none;
  }
  .vlist-type {
    flex: 0 0 auto;
    min-width: 90px;
  }
  .vlist-name {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--color-text, #e2e8f0);
  }
  .vlist-file {
    flex: 0 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--color-text-muted, #94a3b8);
    font-family: 'SF Mono', Menlo, monospace;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
  }
</style>
