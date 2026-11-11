<!-- In-view filter panel (system-explorer.md §1, ui-navigation.md §3):
     200px collapsible left panel INSIDE the Architecture content area —
     not the app sidebar. Category checkboxes (Boundaries, Interfaces,
     Data, Specs) toggle node visibility on the canvas; the Concepts
     section lists predefined concepts discovered from the knowledge graph
     (filter-categories.js) and focuses the canvas on the concept's nodes
     when toggled. -->
<script>
  import { discoverConcepts, categoryCounts } from '../lib/filter-categories.js';

  let {
    nodes = [],
    activeCategories = $bindable({ boundaries: true, interfaces: true, data: true, specs: true }),
    activeConcepts = $bindable([]),
    onClose = () => {},
  } = $props();

  let conceptsOpen = $state(true);

  let concepts = $derived(discoverConcepts(nodes));
  let counts = $derived(categoryCounts(nodes));

  const CATEGORY_LABELS = {
    boundaries: 'Boundaries',
    interfaces: 'Interfaces',
    data: 'Data',
    specs: 'Specs',
  };

  function toggleCategory(id) {
    activeCategories = { ...activeCategories, [id]: !activeCategories[id] };
  }

  function toggleConcept(name) {
    if (activeConcepts.includes(name)) {
      activeConcepts = activeConcepts.filter((c) => c !== name);
    } else {
      activeConcepts = [...activeConcepts, name];
    }
  }

  function clearConcepts() {
    activeConcepts = [];
  }
</script>

<aside
  class="explorer-filter-panel"
  data-testid="explorer-filter-panel"
  aria-label="Architecture filter panel"
>
  <div class="filter-panel-header">
    <h3 class="filter-panel-title">Filter</h3>
    <button
      class="filter-panel-close"
      onclick={onClose}
      aria-label="Collapse filter panel"
      title="Collapse filter panel"
      type="button"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <path d="M15 18l-6-6 6-6"/>
      </svg>
    </button>
  </div>

  <div class="filter-panel-section" role="group" aria-label="Category filters">
    {#each Object.entries(CATEGORY_LABELS) as [id, label] (id)}
      <label class="filter-category" data-testid={`filter-category-${id}`}>
        <input
          type="checkbox"
          checked={activeCategories[id]}
          onchange={() => toggleCategory(id)}
          aria-label={label}
        />
        <span class="filter-category-label">{label}</span>
        <span class="filter-category-count">{counts[id]}</span>
      </label>
    {/each}
  </div>

  <div class="filter-panel-section">
    <button
      class="filter-section-toggle"
      onclick={() => { conceptsOpen = !conceptsOpen; }}
      aria-expanded={conceptsOpen}
      type="button"
    >
      <svg
        viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
        width="12" height="12" aria-hidden="true"
        class="filter-section-chevron"
        class:open={conceptsOpen}
      ><polyline points="9 18 15 12 9 6"/></svg>
      Concepts
    </button>
    {#if conceptsOpen}
      {#if concepts.length === 0}
        <p class="filter-concepts-empty">No concepts discovered in this graph yet.</p>
      {:else}
        <div class="filter-concepts" role="group" aria-label="Concepts">
          {#each concepts as c (c.name)}
            <button
              class="filter-concept-chip"
              class:active={activeConcepts.includes(c.name)}
              onclick={() => toggleConcept(c.name)}
              aria-pressed={activeConcepts.includes(c.name)}
              title={`Focus on the ${c.count} node${c.count === 1 ? '' : 's'} related to ${c.name}`}
              data-testid={`filter-concept-${c.name}`}
              type="button"
            >{c.name} <span class="filter-concept-count">{c.count}</span></button>
          {/each}
        </div>
        {#if activeConcepts.length > 0}
          <button class="filter-concepts-clear" onclick={clearConcepts} type="button">Clear concepts</button>
        {/if}
      {/if}
    {/if}
  </div>
</aside>

<style>
  .explorer-filter-panel {
    width: 200px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px;
    background: rgba(15, 23, 42, 0.55);
    border-right: 1px solid rgba(51, 65, 85, 0.6);
    border-radius: 8px 0 0 8px;
    overflow-y: auto;
    font-family: system-ui, -apple-system, sans-serif;
  }

  .filter-panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .filter-panel-title {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: #94a3b8;
  }

  .filter-panel-close {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 3px;
    border: none;
    border-radius: 5px;
    background: transparent;
    color: #64748b;
    cursor: pointer;
  }
  .filter-panel-close:hover { background: rgba(51, 65, 85, 0.6); color: #e2e8f0; }

  .filter-panel-section {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .filter-category {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 4px 6px;
    border-radius: 6px;
    cursor: pointer;
    color: #cbd5e1;
    font-size: 13px;
  }
  .filter-category:hover { background: rgba(51, 65, 85, 0.45); }
  .filter-category input { accent-color: #3b82f6; margin: 0; }
  .filter-category-label { flex: 1; }
  .filter-category-count {
    font-size: 11px;
    color: #64748b;
    background: rgba(51, 65, 85, 0.6);
    border-radius: 8px;
    padding: 0 6px;
  }

  .filter-section-toggle {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 6px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: #94a3b8;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    cursor: pointer;
  }
  .filter-section-toggle:hover { color: #e2e8f0; }
  .filter-section-chevron { transition: transform 0.15s; }
  .filter-section-chevron.open { transform: rotate(90deg); }

  .filter-concepts { display: flex; flex-wrap: wrap; gap: 4px; }
  .filter-concept-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    border: 1px solid rgba(51, 65, 85, 0.8);
    border-radius: 10px;
    background: transparent;
    color: #94a3b8;
    font-size: 11px;
    cursor: pointer;
  }
  .filter-concept-chip:hover { color: #e2e8f0; border-color: #475569; }
  .filter-concept-chip.active {
    background: rgba(59, 130, 246, 0.2);
    border-color: #3b82f6;
    color: #bfdbfe;
  }
  .filter-concept-count { color: #64748b; font-size: 10px; }

  .filter-concepts-empty {
    margin: 2px 0 0;
    font-size: 11px;
    color: #64748b;
    font-style: italic;
  }

  .filter-concepts-clear {
    margin-top: 4px;
    align-self: flex-start;
    padding: 2px 6px;
    border: none;
    border-radius: 5px;
    background: transparent;
    color: #64748b;
    font-size: 11px;
    cursor: pointer;
    text-decoration: underline;
  }
  .filter-concepts-clear:hover { color: #e2e8f0; }

  @media (prefers-reduced-motion: reduce) {
    .filter-section-chevron { transition: none; }
  }
</style>
