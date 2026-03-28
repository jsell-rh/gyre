<script>
  /**
   * NodeBadge — Vizceral-style ring gauge overlaid on each graph node.
   * Shows span_count, error_rate (as arc %), and mean_duration for a node.
   *
   * Spec ref: ui-layout.md §4 (flow layout — node badges)
   */

  let {
    node = null,     // positioned node {id, x, y, width, height, name}
    metrics = null,  // {span_count, error_rate, mean_duration_us}
  } = $props();

  const RADIUS = 8;
  const CIRCUMFERENCE = 2 * Math.PI * RADIUS; // ≈ 50.27

  // Derived badge position: top-right corner of node
  let bx = $derived(node ? node.x + (node.width ?? 64) - 10 : 0);
  let by = $derived(node ? node.y - 10 : 0);

  let errorRate = $derived(metrics?.error_rate ?? 0);
  let spanCount = $derived(metrics?.span_count ?? 0);
  let meanDuration = $derived(metrics?.mean_duration_us ?? 0);

  // Arc length for error_rate ring
  let errorArc = $derived(Math.max(0, Math.min(1, errorRate)) * CIRCUMFERENCE);
  let ringColor = $derived(errorRate > 0.1 ? 'var(--color-danger)' : 'var(--color-success)');

  let showTooltip = $state(false);

  function formatDuration(us) {
    if (us < 1000) return `${us}µs`;
    if (us < 1_000_000) return `${(us / 1000).toFixed(1)}ms`;
    return `${(us / 1_000_000).toFixed(2)}s`;
  }
</script>

{#if node && metrics && spanCount > 0}
  <!-- svelte-ignore a11y_no_static_element_interactions a11y_no_noninteractive_tabindex -->
  <g
    class="node-badge"
    transform="translate({bx},{by})"
    onmouseenter={() => (showTooltip = true)}
    onmouseleave={() => (showTooltip = false)}
    onfocusin={() => (showTooltip = true)}
    onfocusout={() => (showTooltip = false)}
    tabindex="0"
    role="group"
    aria-label="Node {node.name ?? node.id}: {spanCount} spans, {(errorRate * 100).toFixed(1)}% errors, mean {formatDuration(meanDuration)}"
  >
    <!-- Background ring (border) -->
    <circle
      r={RADIUS}
      fill="color-mix(in srgb, var(--color-bg) 70%, transparent)"
      stroke="var(--color-border)"
      stroke-width="2.5"
    />
    <!-- Error-rate arc -->
    <circle
      r={RADIUS}
      fill="none"
      stroke={ringColor}
      stroke-width="2.5"
      stroke-dasharray="{errorArc} {CIRCUMFERENCE}"
      stroke-linecap="round"
      transform="rotate(-90)"
    />
    <!-- Span count label -->
    <text
      text-anchor="middle"
      dominant-baseline="middle"
      font-size="5"
      fill="var(--color-text)"
      font-family="var(--font-mono)"
      pointer-events="none"
    >{spanCount > 99 ? '99+' : spanCount}</text>

    {#if showTooltip}
      <!-- Tooltip -->
      <g transform="translate(14,-24)">
        <rect
          x="-4" y="-14"
          width="80" height="36"
          rx="3"
          fill="color-mix(in srgb, var(--color-bg) 95%, transparent)"
          stroke="var(--color-border)"
          stroke-width="1"
        />
        <text font-size="7" fill="var(--color-text)" font-family="var(--font-mono)">
          <tspan x="0" dy="0">spans: {spanCount}</tspan>
          <tspan x="0" dy="10">errors: {(errorRate * 100).toFixed(1)}%</tspan>
          <tspan x="0" dy="10">mean: {formatDuration(meanDuration)}</tspan>
        </text>
      </g>
    {/if}
  </g>
{/if}

<style>
  .node-badge {
    cursor: default;
    pointer-events: auto;
  }

  .node-badge:focus-visible circle:first-child {
    stroke: var(--color-focus);
    stroke-width: 3;
  }
</style>
