<!-- dead-component:ok — pre-existing, not imported (baseline for tightened check) -->
<script>
  let {
    columns = [],
    rows = [],
    sortKey = $bindable(null),
    sortDir = $bindable('asc'),
    onrowclick = undefined,
    caption = undefined,
    children,
  } = $props();

  function toggleSort(key) {
    if (sortKey === key) {
      sortDir = sortDir === 'asc' ? 'desc' : 'asc';
    } else {
      sortKey = key;
      sortDir = 'asc';
    }
  }
</script>

<div class="table-wrap">
  <table>
    {#if caption}
      <caption class="sr-only">{caption}</caption>
    {/if}
    <thead>
      <tr>
        {#each columns as col}
          <th
            scope="col"
            class:sortable={col.sortable}
            class:sorted={sortKey === col.key}
            onclick={col.sortable ? () => toggleSort(col.key) : undefined}
            onkeydown={col.sortable ? (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleSort(col.key); } } : undefined}
            tabindex={col.sortable ? 0 : undefined}
            aria-sort={col.sortable ? (sortKey === col.key ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none') : undefined}
            style={col.width ? `width:${col.width}` : ''}
          >
            {col.label}
            {#if col.sortable}
              <span class="sort-icon">
                {#if sortKey === col.key}
                  {sortDir === 'asc' ? '↑' : '↓'}
                {:else}
                  <span class="sort-idle">↕</span>
                {/if}
              </span>
            {/if}
          </th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#if children}
        {@render children()}
      {:else}
        {#each rows as row}
          <tr
            class:clickable={!!onrowclick}
            onclick={onrowclick ? () => onrowclick(row) : undefined}
            onkeydown={onrowclick ? (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onrowclick(row); } } : undefined}
            tabindex={onrowclick ? 0 : undefined}
            role={onrowclick ? 'button' : undefined}
          >
            {#each columns as col}
              <td>{row[col.key] ?? '\u2014'}</td>
            {/each}
          </tr>
        {/each}
      {/if}
    </tbody>
  </table>
</div>

<style>
  .table-wrap {
    overflow-x: auto;
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--text-sm);
    font-family: var(--font-body);
  }

  thead {
    background: var(--color-surface-elevated);
  }

  th {
    padding: var(--space-4) var(--space-4);
    text-align: left;
    font-family: var(--font-display);
    font-weight: 600;
    font-size: var(--text-xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-secondary);
    border-bottom: 1px solid var(--color-border);
    white-space: nowrap;
    user-select: none;
  }

  th.sortable {
    cursor: pointer;
  }

  th.sortable:hover {
    color: var(--color-text);
  }

  th.sorted {
    color: var(--color-text);
  }

  .sort-icon {
    margin-left: var(--space-1);
    font-size: 0.7em;
  }

  .sort-idle {
    opacity: 0.5;
  }

  td {
    padding: var(--space-4) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    color: var(--color-text);
    vertical-align: middle;
  }

  tr:last-child td {
    border-bottom: none;
  }

  tbody tr:hover {
    background: var(--color-surface-elevated);
  }

  tbody tr.clickable {
    cursor: pointer;
  }
  th.sortable:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  tbody tr.clickable:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  th.sortable {
    transition: color var(--transition-fast);
  }

  tbody tr.clickable {
    transition: background var(--transition-fast);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0,0,0,0);
    white-space: nowrap;
    border: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    th.sortable,
    tbody tr.clickable { transition: none; }
  }

</style>
