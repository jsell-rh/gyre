<script>
  import { t } from 'svelte-i18n';

  let {
    visible = false,
    onfilterchange = null,
    nodes = [],
    edges = [],
  } = $props();

  // ── Structural index derived from graph data ───────────────────────
  const BOUNDARY_TYPES = new Set(['module', 'package', 'crate', 'namespace']);
  const INTERFACE_TYPES = new Set(['trait', 'interface', 'protocol', 'abstract_class']);
  const DATA_TYPES = new Set(['struct', 'enum', 'type', 'table', 'model', 'class', 'union']);

  let boundaryNodes = $derived(
    nodes.filter(n => BOUNDARY_TYPES.has((n.node_type ?? '').toLowerCase()))
  );
  let interfaceNodes = $derived(
    nodes.filter(n => INTERFACE_TYPES.has((n.node_type ?? '').toLowerCase()))
  );
  let dataNodes = $derived(
    nodes.filter(n => DATA_TYPES.has((n.node_type ?? '').toLowerCase()))
  );
  let specPaths = $derived(() => {
    const paths = new Set();
    for (const n of nodes) {
      if (n.spec_path) paths.add(n.spec_path);
    }
    // Also check governed_by edges for spec targets
    for (const e of edges) {
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et === 'governed_by') {
        const targetId = e.target_id ?? e.to_node_id ?? e.to;
        const targetNode = nodes.find(nd => nd.id === targetId);
        if (targetNode?.spec_path) paths.add(targetNode.spec_path);
        if (targetNode?.name && !targetNode?.spec_path) paths.add(targetNode.name);
      }
    }
    return [...paths].sort();
  });
  let specPathList = $derived(specPaths());

  // ── Collapsible section state ──────────────────────────────────────
  let boundariesOpen = $state(true);
  let interfacesOpen = $state(true);
  let dataOpen = $state(true);
  let specsOpen = $state(true);
  let advancedOpen = $state(false);

  // ── Advanced filters (legacy category/visibility/churn) ────────────
  const CATEGORY_IDS = ['boundaries', 'interfaces', 'data', 'specs'];
  const CATEGORY_KEYS = {
    boundaries: 'explorer_filter.boundaries',
    interfaces: 'explorer_filter.interfaces',
    data: 'explorer_filter.data',
    specs: 'explorer_filter.specs',
  };

  const VISIBILITY_IDS = ['all', 'public', 'private'];
  const VISIBILITY_KEYS = {
    all: 'explorer_filter.all',
    public: 'explorer_filter.public_only',
    private: 'explorer_filter.private_only',
  };

  let activeCategories = $state(new Set(['boundaries', 'interfaces', 'data', 'specs']));
  let visibility = $state('all');
  let minChurn = $state(0);

  function toggleCategory(id) {
    if (activeCategories.has(id)) {
      activeCategories.delete(id);
    } else {
      activeCategories.add(id);
    }
    activeCategories = new Set(activeCategories);
    emitFilter();
  }

  function emitFilter(extra) {
    onfilterchange?.({
      categories: [...activeCategories],
      visibility: visibility === 'all' ? null : visibility,
      min_churn: minChurn > 0 ? minChurn : null,
      ...extra,
    });
  }

  // ── Item click handlers ────────────────────────────────────────────
  function selectBoundary(node) {
    // Filter to show only this module/package and its children
    emitFilter({ focus_node: node.id, focus_type: 'boundary' });
  }

  function selectInterface(node) {
    emitFilter({ focus_node: node.id, focus_type: 'interface' });
  }

  function selectDataNode(node) {
    emitFilter({ focus_node: node.id, focus_type: 'data' });
  }

  function selectSpec(specPath) {
    emitFilter({ focus_spec: specPath, focus_type: 'spec' });
  }
</script>

{#if visible}
  <div class="filter-panel" role="complementary" aria-label={$t('explorer_filter.title')}>
    <div class="filter-header">
      <span class="filter-title">{$t('explorer_filter.title')}</span>
    </div>

    <!-- Boundaries section -->
    <section class="struct-section">
      <button
        class="section-toggle"
        onclick={() => { boundariesOpen = !boundariesOpen; }}
        aria-expanded={boundariesOpen}
        type="button"
      >
        <span class="toggle-icon" class:open={boundariesOpen}>&#9654;</span>
        <span class="section-label">Boundaries</span>
        <span class="section-badge">{boundaryNodes.length}</span>
      </button>
      {#if boundariesOpen}
        <ul class="struct-list">
          {#if boundaryNodes.length === 0}
            <li class="struct-empty">No modules found</li>
          {:else}
            {#each boundaryNodes as node (node.id)}
              <li>
                <button
                  class="struct-item"
                  onclick={() => selectBoundary(node)}
                  title={node.qualified_name ?? node.name}
                  type="button"
                >
                  <span class="item-icon boundary-icon">M</span>
                  <span class="item-name">{node.name ?? node.qualified_name}</span>
                </button>
              </li>
            {/each}
          {/if}
        </ul>
      {/if}
    </section>

    <!-- Interfaces section -->
    <section class="struct-section">
      <button
        class="section-toggle"
        onclick={() => { interfacesOpen = !interfacesOpen; }}
        aria-expanded={interfacesOpen}
        type="button"
      >
        <span class="toggle-icon" class:open={interfacesOpen}>&#9654;</span>
        <span class="section-label">Interfaces</span>
        <span class="section-badge">{interfaceNodes.length}</span>
      </button>
      {#if interfacesOpen}
        <ul class="struct-list">
          {#if interfaceNodes.length === 0}
            <li class="struct-empty">No traits/interfaces found</li>
          {:else}
            {#each interfaceNodes as node (node.id)}
              <li>
                <button
                  class="struct-item"
                  onclick={() => selectInterface(node)}
                  title={node.qualified_name ?? node.name}
                  type="button"
                >
                  <span class="item-icon interface-icon">I</span>
                  <span class="item-name">{node.name ?? node.qualified_name}</span>
                </button>
              </li>
            {/each}
          {/if}
        </ul>
      {/if}
    </section>

    <!-- Data section -->
    <section class="struct-section">
      <button
        class="section-toggle"
        onclick={() => { dataOpen = !dataOpen; }}
        aria-expanded={dataOpen}
        type="button"
      >
        <span class="toggle-icon" class:open={dataOpen}>&#9654;</span>
        <span class="section-label">Data</span>
        <span class="section-badge">{dataNodes.length}</span>
      </button>
      {#if dataOpen}
        <ul class="struct-list">
          {#if dataNodes.length === 0}
            <li class="struct-empty">No types/tables found</li>
          {:else}
            {#each dataNodes as node (node.id)}
              <li>
                <button
                  class="struct-item"
                  onclick={() => selectDataNode(node)}
                  title={node.qualified_name ?? node.name}
                  type="button"
                >
                  <span class="item-icon data-icon">D</span>
                  <span class="item-name">{node.name ?? node.qualified_name}</span>
                </button>
              </li>
            {/each}
          {/if}
        </ul>
      {/if}
    </section>

    <!-- Specs section -->
    <section class="struct-section">
      <button
        class="section-toggle"
        onclick={() => { specsOpen = !specsOpen; }}
        aria-expanded={specsOpen}
        type="button"
      >
        <span class="toggle-icon" class:open={specsOpen}>&#9654;</span>
        <span class="section-label">Specs</span>
        <span class="section-badge">{specPathList.length}</span>
      </button>
      {#if specsOpen}
        <ul class="struct-list">
          {#if specPathList.length === 0}
            <li class="struct-empty">No spec paths found</li>
          {:else}
            {#each specPathList as sp}
              <li>
                <button
                  class="struct-item"
                  onclick={() => selectSpec(sp)}
                  title={sp}
                  type="button"
                >
                  <span class="item-icon spec-icon">S</span>
                  <span class="item-name">{sp.split('/').pop()}</span>
                </button>
              </li>
            {/each}
          {/if}
        </ul>
      {/if}
    </section>

    <!-- Advanced Filters (legacy) -->
    <section class="struct-section advanced-section">
      <button
        class="section-toggle"
        onclick={() => { advancedOpen = !advancedOpen; }}
        aria-expanded={advancedOpen}
        type="button"
      >
        <span class="toggle-icon" class:open={advancedOpen}>&#9654;</span>
        <span class="section-label">Advanced Filters</span>
      </button>
      {#if advancedOpen}
        <div class="advanced-body">
          <div class="filter-group">
            <h4 class="group-heading">{$t('explorer_filter.categories')}</h4>
            {#each CATEGORY_IDS as catId}
              <label class="filter-checkbox">
                <input
                  type="checkbox"
                  checked={activeCategories.has(catId)}
                  onchange={() => toggleCategory(catId)}
                />
                {$t(CATEGORY_KEYS[catId])}
              </label>
            {/each}
          </div>

          <div class="filter-group">
            <h4 class="group-heading">{$t('explorer_filter.visibility')}</h4>
            {#each VISIBILITY_IDS as vId}
              <label class="filter-radio">
                <input
                  type="radio"
                  name="filter-visibility"
                  value={vId}
                  bind:group={visibility}
                  onchange={emitFilter}
                />
                {$t(VISIBILITY_KEYS[vId])}
              </label>
            {/each}
          </div>

          <div class="filter-group">
            <h4 class="group-heading">{$t('explorer_filter.min_churn')}</h4>
            <div class="churn-wrap">
              <input
                type="range"
                class="churn-slider"
                min="0"
                max="50"
                step="1"
                bind:value={minChurn}
                oninput={emitFilter}
                aria-label={$t('explorer_filter.min_churn')}
              />
              <span class="churn-val">{minChurn}</span>
            </div>
          </div>
        </div>
      {/if}
    </section>
  </div>
{/if}

<style>
  .filter-panel {
    width: 220px;
    flex-shrink: 0;
    background: var(--color-surface-elevated);
    border-right: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    padding: var(--space-3);
    gap: var(--space-1);
  }

  .filter-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-2);
  }

  .filter-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  /* ── Structural sections ──────────────────────────────────────────── */
  .struct-section {
    display: flex;
    flex-direction: column;
  }

  .section-toggle {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-1) var(--space-1);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    color: var(--color-text);
    width: 100%;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .section-toggle:hover {
    background: color-mix(in srgb, var(--color-text) 6%, transparent);
  }

  .section-toggle:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .toggle-icon {
    display: inline-block;
    font-size: 8px;
    transition: transform 0.15s ease;
    color: var(--color-text-muted);
    width: 10px;
    text-align: center;
    flex-shrink: 0;
  }

  .toggle-icon.open {
    transform: rotate(90deg);
  }

  .section-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--color-text-muted);
    flex: 1;
  }

  .section-badge {
    font-size: 10px;
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    background: color-mix(in srgb, var(--color-text-muted) 12%, transparent);
    padding: 0 var(--space-1);
    border-radius: var(--radius-full);
    min-width: 18px;
    text-align: center;
    line-height: 18px;
  }

  .struct-list {
    list-style: none;
    margin: 0;
    padding: 0 0 0 var(--space-3);
    display: flex;
    flex-direction: column;
    max-height: 200px;
    overflow-y: auto;
  }

  .struct-empty {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-style: italic;
    padding: var(--space-1) 0;
  }

  .struct-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 2px var(--space-2);
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    color: var(--color-text);
    width: 100%;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .struct-item:hover {
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    color: var(--color-primary);
  }

  .struct-item:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
  }

  .item-icon {
    width: 16px;
    height: 16px;
    border-radius: 3px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 9px;
    font-weight: 700;
    flex-shrink: 0;
    line-height: 1;
  }

  .boundary-icon {
    background: color-mix(in srgb, #3b82f6 15%, transparent);
    color: #3b82f6;
  }

  .interface-icon {
    background: color-mix(in srgb, #a855f7 15%, transparent);
    color: #a855f7;
  }

  .data-icon {
    background: color-mix(in srgb, #f59e0b 15%, transparent);
    color: #f59e0b;
  }

  .spec-icon {
    background: color-mix(in srgb, #10b981 15%, transparent);
    color: #10b981;
  }

  .item-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    font-family: var(--font-mono);
  }

  /* ── Advanced filters section ─────────────────────────────────────── */
  .advanced-section {
    margin-top: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid var(--color-border);
  }

  .advanced-body {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-2) 0 0 var(--space-2);
  }

  .filter-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .group-heading {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin: 0 0 var(--space-1);
  }

  .filter-checkbox,
  .filter-radio {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--color-text);
    cursor: pointer;
  }

  .filter-checkbox input,
  .filter-radio input {
    accent-color: var(--color-primary);
  }

  .churn-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .churn-slider {
    flex: 1;
    accent-color: var(--color-primary);
  }

  .churn-val {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-text-muted);
    min-width: 20px;
    text-align: right;
  }

  @media (prefers-reduced-motion: reduce) {
    .toggle-icon { transition: none; }
    .struct-item,
    .section-toggle { transition: none; }
  }
</style>
