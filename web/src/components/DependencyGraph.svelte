<script>
  import { api } from '../lib/api.js';
  import Skeleton from '../lib/Skeleton.svelte';
  import EmptyState from '../lib/EmptyState.svelte';
  import Badge from '../lib/Badge.svelte';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let graph = $state(null);
  let loading = $state(true);
  let selected = $state(null);
  let blastRadius = $state(null);
  let blastLoading = $state(false);

  // SVG layout
  let svgW = $state(800);
  let svgH = $state(500);
  let nodePositions = $state({});

  $effect(() => { load(); });

  async function load() {
    loading = true;
    try {
      graph = await api.dependencyGraph();
      layoutGraph();
    } catch (e) {
      showToast('Failed to load dependency graph: ' + e.message, { type: 'error' });
    } finally {
      loading = false;
    }
  }

  function layoutGraph() {
    if (!graph?.nodes?.length) return;
    // Simple circular layout
    const nodes = graph.nodes;
    const n = nodes.length;
    const cx = svgW / 2, cy = svgH / 2;
    const r = Math.min(svgW, svgH) * 0.38;
    const positions = {};
    nodes.forEach((node, i) => {
      const angle = (2 * Math.PI * i) / n - Math.PI / 2;
      positions[node.id] = {
        x: cx + r * Math.cos(angle),
        y: cy + r * Math.sin(angle),
      };
    });
    nodePositions = positions;
  }

  async function selectNode(node) {
    selected = node;
    blastRadius = null;
    blastLoading = true;
    try {
      blastRadius = await api.repoBlastRadius(node.id);
    } catch {
      blastRadius = { affected: [] };
    } finally {
      blastLoading = false;
    }
  }

  function depTypeColor(type) {
    const t = (type ?? '').toLowerCase();
    if (t === 'code') return '#4a9eff';
    if (t === 'spec') return '#a855f7';
    if (t === 'api') return '#22c55e';
    if (t === 'schema') return '#f59e0b';
    return '#6b7280';
  }

  function getPos(id) {
    return nodePositions[id] ?? { x: svgW / 2, y: svgH / 2 };
  }
</script>

<div class="dep-graph-view" aria-busy={loading}>
  <div class="view-header">
    <div>
      <h2>Dependency Graph</h2>
      <p class="subtitle">Tenant-wide repository dependency map</p>
    </div>
    <div class="legend">
      {#each [['Code','#4a9eff'],['Spec','#a855f7'],['API','#22c55e'],['Schema','#f59e0b'],['Manual','#6b7280']] as [label, color]}
        <span class="legend-item">
          <span class="legend-dot" style="background:{color}"></span>
          {label}
        </span>
      {/each}
    </div>
  </div>

  <div class="graph-area">
    {#if loading}
      <div class="loading-box"><Skeleton lines={6} /></div>
    {:else if !graph?.nodes?.length}
      <EmptyState
        title="No dependencies"
        message="No cross-repo dependency edges detected yet. Dependencies are auto-detected from Cargo.toml path dependencies on push."
      />
    {:else}
      <div class="graph-container">
        <svg
          viewBox="0 0 {svgW} {svgH}"
          class="dep-svg"
          role="img"
          aria-label="Dependency graph"
        >
          <!-- Edges -->
          {#each graph.edges as edge}
            {@const from = getPos(edge.from)}
            {@const to = getPos(edge.to)}
            <line
              x1={from.x} y1={from.y}
              x2={to.x} y2={to.y}
              stroke={depTypeColor(edge.dep_type ?? edge.type)}
              stroke-width="1.5"
              stroke-opacity="0.6"
              marker-end="url(#arrow)"
            />
          {/each}

          <!-- Arrow marker -->
          <defs>
            <marker id="arrow" markerWidth="8" markerHeight="8" refX="6" refY="3" orient="auto">
              <path d="M0,0 L0,6 L8,3 z" fill="#6b7280" />
            </marker>
          </defs>

          <!-- Nodes -->
          {#each graph.nodes as node}
            {@const pos = getPos(node.id)}
            {@const isSelected = selected?.id === node.id}
            <g
              class="graph-node"
              transform="translate({pos.x},{pos.y})"
              role="button"
              tabindex="0"
              aria-label="Repo: {node.name ?? node.id}"
              onclick={() => selectNode(node)}
              onkeydown={(e) => e.key === 'Enter' && selectNode(node)}
            >
              <circle
                class="node-circle"
                class:selected={isSelected}
                r={isSelected ? 22 : 18}
                stroke-width={isSelected ? 2 : 1}
              />
              <text
                text-anchor="middle"
                dominant-baseline="middle"
                font-size="9"
                fill="var(--color-text)"
                font-family="var(--font-mono)"
              >
                {(node.name ?? node.id ?? '').substring(0, 10)}
              </text>
            </g>
          {/each}
        </svg>

        <!-- Detail panel -->
        {#if selected}
          <div class="detail-panel">
            <div class="panel-header">
              <span class="panel-title">{selected.name ?? selected.id}</span>
              <button class="close-btn" onclick={() => { selected = null; blastRadius = null; }} aria-label="Close"><span aria-hidden="true">×</span></button>
            </div>
            {#if blastLoading}
              <Skeleton lines={3} />
            {:else if blastRadius}
              <div class="panel-section">
                <div class="panel-label">Blast Radius</div>
                {#if blastRadius.affected?.length === 0}
                  <p class="muted-sm">No transitive dependents.</p>
                {:else}
                  <div class="affected-list">
                    {#each blastRadius.affected as repo}
                      <div class="affected-item">
                        <span class="mono-sm">{repo.name ?? repo.id}</span>
                        {#if repo.dep_type}<Badge variant="default" value={repo.dep_type} />{/if}
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .dep-graph-view { display: flex; flex-direction: column; height: 100%; overflow: hidden; }

  .view-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    padding: var(--space-4) var(--space-6);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-4);
    flex-wrap: wrap;
  }

  .view-header h2 { margin: 0 0 var(--space-1); font-size: var(--text-xl); font-weight: 600; color: var(--color-text); }
  .subtitle { margin: 0; font-size: var(--text-sm); color: var(--color-text-secondary); }

  .legend { display: flex; gap: var(--space-3); align-items: center; flex-wrap: wrap; }
  .legend-item { display: flex; align-items: center; gap: var(--space-1); font-size: var(--text-xs); color: var(--color-text-muted); }
  .legend-dot { width: 8px; height: 8px; border-radius: 50%; }

  .graph-area { flex: 1; overflow: hidden; position: relative; }
  .loading-box { padding: var(--space-6); }

  .graph-container { display: flex; height: 100%; }

  .dep-svg {
    flex: 1;
    width: 100%;
    height: 100%;
    background: var(--color-surface);
    cursor: default;
  }

  .graph-node { cursor: pointer; }
  .graph-node:hover circle { stroke: var(--color-primary); }

  .node-circle {
    fill: var(--color-surface-elevated);
    stroke: var(--color-border-strong);
  }

  .node-circle.selected {
    fill: color-mix(in srgb, var(--color-primary) 15%, transparent);
    stroke: var(--color-primary);
  }

  .detail-panel {
    width: 260px;
    flex-shrink: 0;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
    padding: var(--space-4);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .panel-header { display: flex; justify-content: space-between; align-items: center; }
  .panel-title { font-weight: 600; color: var(--color-text); font-size: var(--text-sm); }
  .close-btn { background: transparent; border: none; color: var(--color-text-muted); cursor: pointer; font-size: var(--text-lg); line-height: 1; transition: color var(--transition-fast); }
  .close-btn:hover { color: var(--color-text); }
  .close-btn:focus-visible { outline: 2px solid var(--color-focus); outline-offset: 2px; border-radius: var(--radius-sm); }

  .panel-section { display: flex; flex-direction: column; gap: var(--space-2); }
  .panel-label { font-size: var(--text-xs); font-weight: 600; text-transform: uppercase; letter-spacing: 0.05em; color: var(--color-text-muted); }

  .affected-list { display: flex; flex-direction: column; gap: var(--space-2); }
  .affected-item { display: flex; align-items: center; gap: var(--space-2); font-size: var(--text-sm); }

  .mono-sm { font-family: var(--font-mono); font-size: var(--text-xs); color: var(--color-text); }
  .muted-sm { font-size: var(--text-sm); color: var(--color-text-muted); margin: 0; }
</style>
