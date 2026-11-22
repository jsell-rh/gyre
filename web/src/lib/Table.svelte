<!-- dead-component:ok — pre-existing, not imported (baseline for tightened check) -->
<script>
  /**
   * Table — compatibility facade composing the shadcn-svelte Table parts
   * (lib/components/ui/table/). Consumed contract unchanged: columns/rows
   * (or a children snippet rendering rows), sortable headers with
   * aria-sort, clickable rows with keyboard activation, sr-only caption.
   * Styling comes from the Tailwind utilities in the ui parts; the
   * bespoke scoped table class layer is retired.
   */
  import {
    Table, TableHeader, TableBody, TableRow, TableHead, TableCell, TableCaption,
  } from './components/ui/table/index.js';
  import { cn } from '$lib/utils.js';

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

<Table class="table-wrap">
  {#if caption}
    <TableCaption>{caption}</TableCaption>
  {/if}
  <TableHeader>
    <TableRow class="hover:bg-transparent">
      {#each columns as col (col.key)}
        <TableHead
          class:cursor-pointer={col.sortable}
          class={cn('transition-colors', col.sortable && 'hover:text-foreground', sortKey === col.key && 'text-foreground')}
          scope="col"
          onclick={col.sortable ? () => toggleSort(col.key) : undefined}
          onkeydown={col.sortable ? (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleSort(col.key); } } : undefined}
          tabindex={col.sortable ? 0 : undefined}
          aria-sort={col.sortable ? (sortKey === col.key ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none') : undefined}
          style={col.width ? `width:${col.width}` : ''}
        >
          {col.label}
          {#if col.sortable}
            <span class="sort-icon ml-1 text-[0.7em]" aria-hidden="true">
              {#if sortKey === col.key}
                {sortDir === 'asc' ? '↑' : '↓'}
              {:else}
                <span class="sort-idle opacity-50">↕</span>
              {/if}
            </span>
          {/if}
        </TableHead>
      {/each}
    </TableRow>
  </TableHeader>
  <TableBody>
    {#if children}
      {@render children()}
    {:else}
      {#each rows as row, i (i)}
        <TableRow
          class={cn(!!onrowclick && 'cursor-pointer')}
          onclick={onrowclick ? () => onrowclick(row) : undefined}
          onkeydown={onrowclick ? (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onrowclick(row); } } : undefined}
          tabindex={onrowclick ? 0 : undefined}
          role={onrowclick ? 'button' : undefined}
        >
          {#each columns as col (col.key)}
            <TableCell>{row[col.key] ?? '\u2014'}</TableCell>
          {/each}
        </TableRow>
      {/each}
    {/if}
  </TableBody>
</Table>
