<script>
  let {
    node = null,
    badges = [],
    queryResult = null,
    position = { x: 0, y: 0 },
  } = $props();

  // Resolve badge text: replace {{count}} with node's match count from queryResult.node_metrics
  function resolveBadgeText(badge, nodeId) {
    const count = queryResult?.node_metrics?.[nodeId] ?? 0;
    return (badge.text ?? '').replace(/\{\{count\}\}/g, String(count));
  }

  // Filter badges that apply to this node based on node_type matching
  let applicableBadges = $derived(
    badges.filter(b => {
      if (!b.node_types) return true;
      return b.node_types.includes(node?.node_type);
    })
  );
</script>

{#if node && applicableBadges.length > 0}
  {#each applicableBadges as badge, i}
    <div
      class="node-badge"
      style="left: {position.x}px; top: {position.y + (i * 18)}px; background: {badge.color ?? 'rgba(15, 15, 26, 0.9)'}; border-color: {badge.borderColor ?? '#334155'};"
      role="status"
      aria-label="Badge: {resolveBadgeText(badge, node.id)}"
    >
      {resolveBadgeText(badge, node.id)}
    </div>
  {/each}
{/if}

<style>
  .node-badge {
    position: absolute; z-index: 30;
    padding: 1px 5px;
    border: 1px solid #334155;
    border-radius: 4px;
    font-size: 9px;
    font-weight: 600;
    color: #e2e8f0;
    font-family: 'SF Mono', Menlo, monospace;
    white-space: nowrap;
    pointer-events: none;
    line-height: 14px;
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
  }
</style>
