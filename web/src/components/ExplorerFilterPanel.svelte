<!-- dead-component:ok — pre-existing, not imported (baseline for tightened check) -->
<script>
  import { t } from 'svelte-i18n';

  let {
    visible = false,
    onfilterchange = null,
    nodes = [],
    edges = [],
  } = $props();

  // ── Node type classification ──────────────────────────────────────
  const BOUNDARY_TYPES = new Set(['module', 'package', 'crate', 'namespace']);
  const INTERFACE_TYPES = new Set(['trait', 'interface', 'protocol', 'abstract_class', 'endpoint', 'handler']);
  const DATA_TYPES = new Set(['struct', 'enum', 'type', 'table', 'model', 'class', 'union', 'field']);
  const SPEC_TYPES = new Set(['spec']);

  // Sub-type grouping labels for each section
  const INTERFACE_GROUPS = {
    traits: new Set(['trait', 'interface', 'protocol', 'abstract_class']),
    endpoints: new Set(['endpoint', 'handler']),
  };
  const DATA_GROUPS = {
    types: new Set(['struct', 'enum', 'type', 'class', 'union']),
    tables: new Set(['table', 'model']),
    fields: new Set(['field']),
  };

  // ── Single-pass categorization ────────────────────────────────────
  let categorized = $derived.by(() => {
    const boundaries = [];
    const interfaces = { traits: [], endpoints: [] };
    const data = { types: [], tables: [], fields: [] };
    const specNodes = [];
    const specPaths = new Set();

    // Build governed-by map: nodeId -> set of spec paths
    const governedBy = new Map();
    const nodeById = new Map(nodes.map(n => [n.id, n]));

    for (const e of edges) {
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et === 'governed_by') {
        const sourceId = e.source_id ?? e.from_node_id ?? e.from;
        const targetId = e.target_id ?? e.to_node_id ?? e.to;
        const targetNode = nodeById.get(targetId);
        const specPath = targetNode?.spec_path ?? targetNode?.name;
        if (specPath) {
          if (!governedBy.has(sourceId)) governedBy.set(sourceId, new Set());
          governedBy.get(sourceId).add(specPath);
        }
      }
    }

    for (const n of nodes) {
      const nt = (n.node_type ?? '').toLowerCase();

      if (BOUNDARY_TYPES.has(nt)) {
        boundaries.push(n);
      }

      if (INTERFACE_TYPES.has(nt)) {
        if (INTERFACE_GROUPS.endpoints.has(nt)) {
          interfaces.endpoints.push(n);
        } else {
          interfaces.traits.push(n);
        }
      }

      if (DATA_TYPES.has(nt)) {
        if (DATA_GROUPS.tables.has(nt)) {
          data.tables.push(n);
        } else if (DATA_GROUPS.fields.has(nt)) {
          data.fields.push(n);
        } else {
          data.types.push(n);
        }
      }

      if (SPEC_TYPES.has(nt)) {
        specNodes.push(n);
      }

      if (n.spec_path) specPaths.add(n.spec_path);
    }

    // If no explicit spec nodes, derive from governed_by edges
    if (specNodes.length === 0) {
      for (const e of edges) {
        const et = (e.edge_type ?? e.type ?? '').toLowerCase();
        if (et === 'governed_by') {
          const targetId = e.target_id ?? e.to_node_id ?? e.to;
          const targetNode = nodeById.get(targetId);
          if (targetNode && !specNodes.some(s => s.id === targetNode.id)) {
            specNodes.push(targetNode);
          }
        }
      }
    }

    // Collect all spec paths for fallback display
    for (const sp of specPaths) {
      if (!specNodes.some(s => (s.spec_path ?? s.name) === sp)) {
        specNodes.push({ id: `spec:${sp}`, name: sp.split('/').pop(), spec_path: sp, node_type: 'spec' });
      }
    }

    // Count governance stats
    const governedCount = governedBy.size;
    const totalNonSpec = nodes.filter(n => !SPEC_TYPES.has((n.node_type ?? '').toLowerCase())).length;

    return {
      boundaries,
      interfaces,
      data,
      specNodes: specNodes.sort((a, b) => (a.name ?? '').localeCompare(b.name ?? '')),
      governedBy,
      governedCount,
      totalNonSpec,
    };
  });

  let boundaryNodes = $derived(categorized.boundaries);
  let interfaceGroups = $derived(categorized.interfaces);
  let dataGroups = $derived(categorized.data);
  let specNodes = $derived(categorized.specNodes);
  let governedBy = $derived(categorized.governedBy);

  // Total counts for badges
  let interfaceCount = $derived(interfaceGroups.traits.length + interfaceGroups.endpoints.length);
  let dataCount = $derived(dataGroups.types.length + dataGroups.tables.length + dataGroups.fields.length);

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

  let filterDebounceTimer = null;
  function emitFilter(extra) {
    const payload = {
      categories: [...activeCategories],
      visibility: visibility === 'all' ? null : visibility,
      min_churn: minChurn > 0 ? minChurn : null,
      ...extra,
    };
    // Debounce rapid filter changes (e.g. slider dragging)
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    filterDebounceTimer = setTimeout(() => {
      onfilterchange?.(payload);
      filterDebounceTimer = null;
    }, 50);
  }

  // ── Item click handlers ────────────────────────────────────────────
  function selectBoundary(node) {
    emitFilter({ focus_node: node.id, focus_type: 'boundary' });
  }

  function selectInterface(node) {
    emitFilter({ focus_node: node.id, focus_type: 'interface' });
  }

  function selectDataNode(node) {
    emitFilter({ focus_node: node.id, focus_type: 'data' });
  }

  function selectSpec(node) {
    const specPath = node.spec_path ?? node.name;
    emitFilter({ focus_node: node.id, focus_spec: specPath, focus_type: 'spec' });
  }

  // ── Governance helpers ─────────────────────────────────────────────
  function getGovernanceStatus(node) {
    if (governedBy.has(node.id)) return 'governed';
    if (node.spec_path) return 'governed';
    return 'ungoverned';
  }

  function getNodeTypeIcon(nt) {
    const t = (nt ?? '').toLowerCase();
    if (t === 'module' || t === 'package') return 'M';
    if (t === 'crate') return 'C';
    if (t === 'namespace') return 'N';
    if (t === 'trait' || t === 'interface' || t === 'protocol') return 'T';
    if (t === 'abstract_class') return 'A';
    if (t === 'endpoint' || t === 'handler') return 'E';
    if (t === 'struct') return 'S';
    if (t === 'enum') return 'E';
    if (t === 'table' || t === 'model') return 'T';
    if (t === 'field') return 'F';
    if (t === 'class') return 'C';
    if (t === 'union' || t === 'type') return 'U';
    if (t === 'spec') return 'S';
    return '?';
  }
</script>

{#if visible}
  <div class="filter-panel" role="complementary" aria-label={$t('explorer_filter.title')}>
    <div class="filter-header">
      <span class="filter-title">{$t('explorer_filter.title')}</span>
      {#if categorized.totalNonSpec > 0}
        <span class="governance-summary" title="{categorized.governedCount}/{categorized.totalNonSpec} nodes governed by specs">
          {Math.round((categorized.governedCount / categorized.totalNonSpec) * 100)}%
        </span>
      {/if}
    </div>

    <!-- ═══ Boundaries ═══ -->
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
                  <span class="item-icon boundary-icon">{getNodeTypeIcon(node.node_type)}</span>
                  <span class="item-name">{node.name ?? node.qualified_name}</span>
                  <span class="gov-dot" class:governed={getGovernanceStatus(node) === 'governed'}></span>
                </button>
              </li>
            {/each}
          {/if}
        </ul>
      {/if}
    </section>

    <!-- ═══ Interfaces ═══ -->
    <section class="struct-section">
      <button
        class="section-toggle"
        onclick={() => { interfacesOpen = !interfacesOpen; }}
        aria-expanded={interfacesOpen}
        type="button"
      >
        <span class="toggle-icon" class:open={interfacesOpen}>&#9654;</span>
        <span class="section-label">Interfaces</span>
        <span class="section-badge">{interfaceCount}</span>
      </button>
      {#if interfacesOpen}
        {#if interfaceGroups.traits.length > 0}
          <div class="subgroup-label">Traits</div>
          <ul class="struct-list">
            {#each interfaceGroups.traits as node (node.id)}
              <li>
                <button
                  class="struct-item"
                  onclick={() => selectInterface(node)}
                  title={node.qualified_name ?? node.name}
                  type="button"
                >
                  <span class="item-icon interface-icon">{getNodeTypeIcon(node.node_type)}</span>
                  <span class="item-name">{node.name ?? node.qualified_name}</span>
                  <span class="gov-dot" class:governed={getGovernanceStatus(node) === 'governed'}></span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if interfaceGroups.endpoints.length > 0}
          <div class="subgroup-label">Endpoints</div>
          <ul class="struct-list">
            {#each interfaceGroups.endpoints as node (node.id)}
              <li>
                <button
                  class="struct-item"
                  onclick={() => selectInterface(node)}
                  title={node.qualified_name ?? node.name}
                  type="button"
                >
                  <span class="item-icon endpoint-icon">{getNodeTypeIcon(node.node_type)}</span>
                  <span class="item-name">{node.name ?? node.qualified_name}</span>
                  <span class="gov-dot" class:governed={getGovernanceStatus(node) === 'governed'}></span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if interfaceCount === 0}
          <ul class="struct-list">
            <li class="struct-empty">No traits or endpoints found</li>
          </ul>
        {/if}
      {/if}
    </section>

    <!-- ═══ Data ═══ -->
    <section class="struct-section">
      <button
        class="section-toggle"
        onclick={() => { dataOpen = !dataOpen; }}
        aria-expanded={dataOpen}
        type="button"
      >
        <span class="toggle-icon" class:open={dataOpen}>&#9654;</span>
        <span class="section-label">Data</span>
        <span class="section-badge">{dataCount}</span>
      </button>
      {#if dataOpen}
        {#if dataGroups.types.length > 0}
          <div class="subgroup-label">Types</div>
          <ul class="struct-list">
            {#each dataGroups.types as node (node.id)}
              <li>
                <button
                  class="struct-item"
                  onclick={() => selectDataNode(node)}
                  title={node.qualified_name ?? node.name}
                  type="button"
                >
                  <span class="item-icon data-icon">{getNodeTypeIcon(node.node_type)}</span>
                  <span class="item-name">{node.name ?? node.qualified_name}</span>
                  <span class="gov-dot" class:governed={getGovernanceStatus(node) === 'governed'}></span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if dataGroups.tables.length > 0}
          <div class="subgroup-label">Tables</div>
          <ul class="struct-list">
            {#each dataGroups.tables as node (node.id)}
              <li>
                <button
                  class="struct-item"
                  onclick={() => selectDataNode(node)}
                  title={node.qualified_name ?? node.name}
                  type="button"
                >
                  <span class="item-icon table-icon">{getNodeTypeIcon(node.node_type)}</span>
                  <span class="item-name">{node.name ?? node.qualified_name}</span>
                  <span class="gov-dot" class:governed={getGovernanceStatus(node) === 'governed'}></span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if dataGroups.fields.length > 0}
          <div class="subgroup-label">Fields</div>
          <ul class="struct-list">
            {#each dataGroups.fields as node (node.id)}
              <li>
                <button
                  class="struct-item"
                  onclick={() => selectDataNode(node)}
                  title={node.qualified_name ?? node.name}
                  type="button"
                >
                  <span class="item-icon field-icon">{getNodeTypeIcon(node.node_type)}</span>
                  <span class="item-name">{node.name ?? node.qualified_name}</span>
                  <span class="gov-dot" class:governed={getGovernanceStatus(node) === 'governed'}></span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        {#if dataCount === 0}
          <ul class="struct-list">
            <li class="struct-empty">No types or tables found</li>
          </ul>
        {/if}
      {/if}
    </section>

    <!-- ═══ Specs ═══ -->
    <section class="struct-section">
      <button
        class="section-toggle"
        onclick={() => { specsOpen = !specsOpen; }}
        aria-expanded={specsOpen}
        type="button"
      >
        <span class="toggle-icon" class:open={specsOpen}>&#9654;</span>
        <span class="section-label">Specs</span>
        <span class="section-badge">{specNodes.length}</span>
      </button>
      {#if specsOpen}
        <ul class="struct-list">
          {#if specNodes.length === 0}
            <li class="struct-empty">No specs found</li>
          {:else}
            {#each specNodes as node (node.id)}
              {@const specPath = node.spec_path ?? node.name}
              {@const govCount = [...(categorized.governedBy.entries())].filter(([, paths]) => paths.has(specPath)).length}
              <li>
                <button
                  class="struct-item spec-item"
                  onclick={() => selectSpec(node)}
                  title="{specPath} ({govCount} governed node{govCount !== 1 ? 's' : ''})"
                  type="button"
                >
                  <span class="item-icon spec-icon">S</span>
                  <span class="item-name">{(node.name ?? specPath).split('/').pop()}</span>
                  {#if govCount > 0}
                    <span class="gov-count" title="{govCount} nodes governed">{govCount}</span>
                  {/if}
                </button>
              </li>
            {/each}
          {/if}
        </ul>
      {/if}
    </section>

    <!-- ═══ Advanced Filters ═══ -->
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
    background: rgba(15, 15, 26, 0.95);
    border-right: 1px solid #334155;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    padding: var(--space-3);
    gap: var(--space-1);
    color: #e2e8f0;
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
    color: #e2e8f0;
  }

  .governance-summary {
    font-size: 10px;
    font-weight: 600;
    font-family: var(--font-mono);
    color: #10b981;
    background: color-mix(in srgb, #10b981 12%, transparent);
    padding: 1px 6px;
    border-radius: var(--radius-full);
    line-height: 16px;
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
    color: #e2e8f0;
    width: 100%;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .section-toggle:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  .section-toggle:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .toggle-icon {
    display: inline-block;
    font-size: 8px;
    transition: transform 0.15s ease;
    color: #94a3b8;
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
    color: #94a3b8;
    flex: 1;
  }

  .section-badge {
    font-size: 10px;
    font-weight: 600;
    font-family: var(--font-mono);
    color: #94a3b8;
    background: rgba(148, 163, 184, 0.12);
    padding: 0 var(--space-1);
    border-radius: var(--radius-full);
    min-width: 18px;
    text-align: center;
    line-height: 18px;
  }

  .subgroup-label {
    font-size: 10px;
    font-weight: 500;
    color: #64748b;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: var(--space-1) 0 2px var(--space-4);
    margin-top: 2px;
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
    color: #64748b;
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
    color: #e2e8f0;
    width: 100%;
    text-align: left;
    transition: background var(--transition-fast);
  }

  .struct-item:hover {
    background: rgba(99, 102, 241, 0.1);
    color: #a5b4fc;
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
    background: rgba(59, 130, 246, 0.15);
    color: #3b82f6;
  }

  .interface-icon {
    background: rgba(168, 85, 247, 0.15);
    color: #a855f7;
  }

  .endpoint-icon {
    background: rgba(236, 72, 153, 0.15);
    color: #ec4899;
  }

  .data-icon {
    background: rgba(245, 158, 11, 0.15);
    color: #f59e0b;
  }

  .table-icon {
    background: rgba(234, 179, 8, 0.15);
    color: #eab308;
  }

  .field-icon {
    background: rgba(251, 191, 36, 0.12);
    color: #fbbf24;
  }

  .spec-icon {
    background: rgba(16, 185, 129, 0.15);
    color: #10b981;
  }

  .item-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 1;
    font-family: var(--font-mono);
  }

  /* ── Governance indicators ───────────────────────────────────────── */
  .gov-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    background: #ef4444;
    opacity: 0.6;
  }

  .gov-dot.governed {
    background: #10b981;
    opacity: 0.8;
  }

  .gov-count {
    font-size: 9px;
    font-weight: 600;
    font-family: var(--font-mono);
    color: #94a3b8;
    background: rgba(148, 163, 184, 0.1);
    padding: 0 4px;
    border-radius: var(--radius-full);
    line-height: 14px;
    flex-shrink: 0;
  }

  /* ── Advanced filters section ─────────────────────────────────────── */
  .advanced-section {
    margin-top: var(--space-2);
    padding-top: var(--space-2);
    border-top: 1px solid #334155;
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
    color: #94a3b8;
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
    color: #e2e8f0;
    cursor: pointer;
  }

  .filter-checkbox input,
  .filter-radio input {
    accent-color: #6366f1;
  }

  .churn-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .churn-slider {
    flex: 1;
    accent-color: #6366f1;
  }

  .churn-val {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: #94a3b8;
    min-width: 20px;
    text-align: right;
  }

  @media (prefers-reduced-motion: reduce) {
    .toggle-icon { transition: none; }
    .struct-item,
    .section-toggle { transition: none; }
  }
</style>
