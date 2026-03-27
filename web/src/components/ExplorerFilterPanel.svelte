<script>
  let {
    visible = false,
    onFilterChange = null,
  } = $props();

  // Category filters matching system-explorer.md categories
  const CATEGORIES = [
    { id: 'boundaries', label: 'Boundaries' },
    { id: 'interfaces', label: 'Interfaces' },
    { id: 'data', label: 'Data' },
    { id: 'specs', label: 'Specs' },
  ];

  const VISIBILITIES = [
    { id: 'all', label: 'All' },
    { id: 'public', label: 'Public only' },
    { id: 'private', label: 'Private only' },
  ];

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

  function emitFilter() {
    onFilterChange?.({
      categories: [...activeCategories],
      visibility: visibility === 'all' ? null : visibility,
      min_churn: minChurn > 0 ? minChurn : null,
    });
  }
</script>

{#if visible}
  <div class="filter-panel" role="complementary" aria-label="Explorer filter panel">
    <div class="filter-header">
      <span class="filter-title">Filters</span>
    </div>

    <section class="filter-section">
      <h4 class="section-heading">Categories</h4>
      {#each CATEGORIES as cat}
        <label class="filter-checkbox">
          <input
            type="checkbox"
            checked={activeCategories.has(cat.id)}
            onchange={() => toggleCategory(cat.id)}
          />
          {cat.label}
        </label>
      {/each}
    </section>

    <section class="filter-section">
      <h4 class="section-heading">Visibility</h4>
      {#each VISIBILITIES as v}
        <label class="filter-radio">
          <input
            type="radio"
            name="filter-visibility"
            value={v.id}
            bind:group={visibility}
            onchange={emitFilter}
          />
          {v.label}
        </label>
      {/each}
    </section>

    <section class="filter-section">
      <h4 class="section-heading">Min churn (30d)</h4>
      <div class="churn-wrap">
        <input
          type="range"
          class="churn-slider"
          min="0"
          max="50"
          step="1"
          bind:value={minChurn}
          oninput={emitFilter}
          aria-label="Minimum churn count"
        />
        <span class="churn-val">{minChurn}</span>
      </div>
    </section>
  </div>
{/if}

<style>
  .filter-panel {
    width: 200px;
    flex-shrink: 0;
    background: var(--color-surface-elevated);
    border-right: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    padding: var(--space-3);
    gap: var(--space-3);
  }

  .filter-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .filter-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .filter-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .section-heading {
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
    font-size: var(--text-sm);
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
</style>
