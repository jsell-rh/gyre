<script>
  import { ghostOpacity, GHOST_NODE_W, GHOST_NODE_H } from './ghostOverlay.js';
  import { t } from 'svelte-i18n';

  /**
   * GhostOverlay — the §3 ghost overlay SVG layer
   * (system-explorer.md "Ghost Overlay Visual Language").
   *
   * Rendered inside a parent SVG canvas (ArchPreviewCanvas) on top of the
   * regular nodes. Implements the spec's visual language exactly:
   *
   *   | Visual                          | Meaning                          |
   *   |---------------------------------|----------------------------------|
   *   | Dotted outline, green fill      | New element predicted to add     |
   *   | Yellow highlight on node        | Existing element predicted change|
   *   | Red strikethrough on node       | Existing element predicted remove|
   *   | Dotted edge, green              | New relationship predicted       |
   *   | Pulsing border                  | Currently being previewed        |
   *   | Solid border (after preview)    | Confirmed by real implementation |
   *
   * Ghost opacity scales with the prediction's confidence score
   * (ghostOverlay.js ghostOpacity — 0.3 at 50%, 1.0 at 100%).
   *
   * Ghost nodes are interactive (hover/focus opens a tooltip with the
   * prediction reason) but never trigger canvas navigation — the overlay
   * carries no click handlers by design.
   *
   * Props:
   *   ghosts   — normalized ghosts (ghostOverlay.js normalizeGhosts)
   *   positions — ghost id → {x, y} (ghostOverlay.js positionGhosts)
   *   edges    — [{ id, from: {x,y}, to: {x,y} }] (ghostEdges)
   *   mode     — 'predicted' (fast tier) | 'previewing' (agent running —
   *              pulsing border) | 'confirmed' (preview complete — solid
   *              border, full opacity)
   *   nodeW/nodeH — ghost body size (defaults match the canvas nodes)
   */
  let {
    ghosts = [],
    positions = {},
    edges = [],
    mode = 'predicted',
    nodeW = GHOST_NODE_W,
    nodeH = GHOST_NODE_H,
  } = $props();

  let hovered = $state(null);

  // §3 action colors: green=add, yellow=modify, red=remove.
  const STROKE = { add: '#22c55e', modify: '#eab308', remove: '#ef4444' };
  const FILL = {
    add: 'rgba(34,197,94,0.16)',
    modify: 'rgba(234,179,8,0.16)',
    remove: 'rgba(239,68,68,0.12)',
  };

  function posOf(id) {
    return positions[id] ?? { x: 120, y: 80 };
  }

  function label(ghost) {
    return String(ghost.name).substring(0, 14);
  }

  /** Simple word wrap for tooltip text lines. */
  function wrap(text, maxChars = 36) {
    const words = String(text).split(/\s+/).filter(Boolean);
    const lines = [];
    let line = '';
    for (const word of words) {
      if (line === '') {
        line = word;
      } else if ((line + ' ' + word).length <= maxChars) {
        line += ' ' + word;
      } else {
        lines.push(line);
        line = word;
      }
    }
    if (line !== '') lines.push(line);
    return lines.length ? lines : [''];
  }

  function tooltipLines(ghost) {
    const pct = Math.round(clamp(ghost.confidence) * 100);
    const lines = [`${ghost.name} — ${ghost.action} (${pct}%)`];
    if (ghost.reason) lines.push(...wrap(ghost.reason));
    for (const change of ghost.changes ?? []) {
      lines.push(...wrap(`• ${change}`));
    }
    return lines.slice(0, 8);
  }

  function clamp(c) {
    const n = typeof c === 'number' ? c : Number(c);
    return Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : 0.5;
  }

  function tooltipWidth(lines) {
    return Math.max(...lines.map((l) => l.length)) * 5.4 + 14;
  }
</script>

<g
  class="ghost-overlay"
  data-testid="ghost-overlay"
  data-mode={mode}
  data-ghost-count={ghosts.length}
  aria-label={$t('explorer_canvas.ghost_overlay_aria')}
>
  <!-- Predicted new relationships: dotted green edges -->
  {#each edges as edge (edge.id)}
    <line
      class="ghost-edge"
      data-testid="ghost-edge"
      x1={edge.from.x}
      y1={edge.from.y}
      x2={edge.to.x}
      y2={edge.to.y}
      stroke="#22c55e"
      stroke-width="1.5"
      stroke-dasharray="2 4"
      stroke-opacity="0.85"
      pointer-events="none"
      aria-hidden="true" />
  {/each}

  <!-- Ghost nodes: dotted outline + action-colored semi-transparent fill,
       opacity scaled by confidence; strikethrough for removes; pulsing
       border while an agent preview runs; solid border once confirmed. -->
  {#each ghosts as ghost (ghost.id)}
    {@const pos = posOf(ghost.id)}
    {@const color = STROKE[ghost.action] ?? '#94a3b8'}
    {@const opacity = ghostOpacity(ghost.confidence)}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <g
      class="ghost-node"
      class:ghost-previewing={mode === 'previewing'}
      class:ghost-confirmed={mode === 'confirmed'}
      data-testid="ghost-node"
      data-action={ghost.action}
      data-node-id={ghost.nodeId ?? ghost.name}
      data-confidence={ghost.confidence}
      transform="translate({pos.x},{pos.y})"
      role="img"
      tabindex="0"
      aria-label={$t('explorer_canvas.ghost_node_aria', {
        values: { action: ghost.action, name: ghost.name, reason: ghost.reason },
      })}
      onmouseenter={() => (hovered = ghost)}
      onmouseleave={() => (hovered = null)}
      onfocus={() => (hovered = ghost)}
      onblur={() => (hovered = null)}>
      <rect
        class="ghost-border"
        x={-nodeW / 2 - 4}
        y={-nodeH / 2 - 4}
        width={nodeW + 8}
        height={nodeH + 8}
        rx="5"
        fill={FILL[ghost.action] ?? 'transparent'}
        fill-opacity={opacity}
        stroke={color}
        stroke-width="2"
        stroke-dasharray="2 3"
        opacity={opacity} />
      <text
        class="ghost-label"
        text-anchor="middle"
        dominant-baseline="middle"
        font-size="9"
        fill={color}
        pointer-events="none"
        style="font-family: var(--font-mono, monospace); user-select: none">
        {label(ghost)}
      </text>
      {#if ghost.action === 'remove'}
        <line
          class="ghost-strikethrough"
          data-testid="ghost-strikethrough"
          x1={-nodeW / 2 - 2}
          y1="0"
          x2={nodeW / 2 + 2}
          y2="0"
          stroke="#ef4444"
          stroke-width="2"
          pointer-events="none"
          aria-hidden="true" />
      {/if}
    </g>
  {/each}

  <!-- Tooltip: prediction reason, confidence, changes. Hover/focus only —
       never a click target (ghosts do not trigger navigation). -->
  {#if hovered}
    {@const pos = posOf(hovered.id)}
    {@const lines = tooltipLines(hovered)}
    {@const width = tooltipWidth(lines)}
    <g
      class="ghost-tooltip"
      data-testid="ghost-tooltip"
      transform="translate({pos.x + nodeW / 2 + 12},{pos.y - nodeH - 6})"
      pointer-events="none"
      role="tooltip">
      <rect
        class="ghost-tooltip-box"
        x="0"
        y="0"
        width={width}
        height={lines.length * 12 + 10}
        rx="4" />
      {#each lines as line, i (i)}
        <text class="ghost-tooltip-line" x="7" y={16 + i * 12}>{line}</text>
      {/each}
    </g>
  {/if}
</g>

<style>
  .ghost-node {
    cursor: help;
  }

  .ghost-node:focus-visible {
    outline: none;
  }

  .ghost-node:focus-visible .ghost-border {
    stroke-width: 3;
  }

  /* §3 "Pulsing border — currently being previewed (agent running)". */
  .ghost-node.ghost-previewing .ghost-border {
    animation: ghost-pulse 1.3s ease-in-out infinite;
  }

  @keyframes ghost-pulse {
    0%,
    100% {
      stroke-width: 2;
      stroke-opacity: 0.7;
    }
    50% {
      stroke-width: 4.5;
      stroke-opacity: 1;
    }
  }

  /* §3 "Solid border (after preview) — confirmed by actual implementation":
     the dotted prediction outline becomes solid and full-opacity. CSS
     overrides the stroke-dasharray/opacity presentation attributes. */
  .ghost-node.ghost-confirmed .ghost-border {
    stroke-dasharray: none;
    animation: none;
    opacity: 1;
    fill-opacity: 0.3;
  }

  .ghost-edge {
    pointer-events: none;
  }

  .ghost-tooltip-box {
    fill: #0b1220;
    stroke: #334155;
    stroke-width: 1;
  }

  .ghost-tooltip-line {
    fill: #e2e8f0;
    font-size: 9px;
    font-family: var(--font-mono, monospace);
  }
</style>
