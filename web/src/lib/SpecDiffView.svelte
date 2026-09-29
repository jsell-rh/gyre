<script>
  import { t } from 'svelte-i18n';

  /**
   * SpecDiffView — side-by-side line diff between two spec versions.
   *
   * Shared by SpecConflictDialog (save-time 409 dialog) and the Inbox
   * `SpecConflict` card (HSI §7 item 3 — "The conflict appears in both users'
   * Inboxes with a diff view"). Rows come from the server's LCS line diff:
   *   context → both columns show the line
   *   remove  → left (current/server) only
   *   add     → right (yours) only
   *
   * Props:
   *   diff         — array of { op: 'context'|'remove'|'add', text: string }
   *   currentLabel — override for the left column header (defaults to i18n)
   *   yoursLabel   — override for the right column header (defaults to i18n)
   */
  let { diff = [], currentLabel = null, yoursLabel = null } = $props();

  let rows = $derived(buildRows(diff ?? []));

  function buildRows(d) {
    return d.map((x) => {
      if (x.op === 'remove') return { left: x.text, right: null, op: 'remove' };
      if (x.op === 'add') return { left: null, right: x.text, op: 'add' };
      return { left: x.text, right: x.text, op: 'context' };
    });
  }
</script>

{#if rows.length > 0}
  <div class="conflict-diff" role="table" aria-label={$t('spec_conflict.diff_label')} data-testid="spec-diff-view">
    <div class="diff-col-headers" role="row">
      <span class="diff-col-header" role="columnheader">{currentLabel ?? $t('spec_conflict.current_version')}</span>
      <span class="diff-col-header" role="columnheader">{yoursLabel ?? $t('spec_conflict.your_version')}</span>
    </div>
    <div class="diff-body">
      {#each rows as row}
        <div class="diff-row" role="row">
          <span class="diff-cell diff-left diff-{row.op}" role="cell">{row.left ?? ''}</span>
          <span class="diff-cell diff-right diff-{row.op}" role="cell">{row.right ?? ''}</span>
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .conflict-diff {
    margin: 0.75rem 0;
    border: 1px solid var(--border, #e2e2e2);
    border-radius: 6px;
    overflow: auto;
    flex: 1;
    max-height: 60vh;
    font-family: var(--mono, ui-monospace, monospace);
    font-size: 0.8rem;
  }
  .diff-col-headers,
  .diff-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
  }
  .diff-col-headers {
    position: sticky;
    top: 0;
    background: var(--surface-alt, #f5f5f5);
    border-bottom: 1px solid var(--border, #e2e2e2);
    font-weight: 600;
  }
  .diff-col-header {
    padding: 0.35rem 0.6rem;
  }
  .diff-col-header:first-child {
    border-right: 1px solid var(--border, #e2e2e2);
  }
  .diff-cell {
    padding: 0.1rem 0.6rem;
    white-space: pre-wrap;
    word-break: break-word;
    border-right: 1px solid var(--border, #eee);
    min-height: 1.2em;
  }
  .diff-cell.diff-remove.diff-left {
    background: rgba(220, 50, 50, 0.14);
  }
  .diff-cell.diff-add.diff-right {
    background: rgba(50, 160, 80, 0.16);
  }
</style>
