<script>
  import { t } from 'svelte-i18n';
  import Button from './Button.svelte';
  import SpecDiffView from './SpecDiffView.svelte';
  import { toastSuccess } from './toast.svelte.js';

  /**
   * SpecConflictDialog — 409 Conflict resolution dialog (HSI §7 Conflict Prevention).
   *
   * Shown when a spec save is rejected because the spec's `current_sha` advanced
   * since the user loaded it (another editor merged a change). Renders a
   * side-by-side line diff between the server's current version and the user's
   * submitted version, and offers resolution actions.
   *
   * Props:
   *   conflict     — the 409 response body:
   *                  { spec_path, base_sha, current_sha, current_content,
   *                    submitted_content, diff: [{op, text}] }
   *   onOverwrite  — () => void  — force-save with the latest sha
   *   onDiscard    — () => void  — discard local changes, reload server version
   *   onClose      — () => void  — dismiss the dialog without resolving
   */
  let { conflict = null, onOverwrite = undefined, onDiscard = undefined, onClose = undefined } = $props();

  // Diff rows are built by the shared SpecDiffView component.

  async function copyMine() {
    const text = conflict?.submitted_content ?? '';
    try {
      await navigator.clipboard.writeText(text);
      toastSuccess($t('spec_conflict.copied'));
    } catch {
      /* clipboard unavailable */
    }
  }

  function handleKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      onClose?.();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if conflict}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="conflict-backdrop"
    role="presentation"
    onclick={(e) => { if (e.target === e.currentTarget) onClose?.(); }}
    data-testid="spec-conflict-backdrop"
  >
    <div
      class="conflict-dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="spec-conflict-title"
      data-testid="spec-conflict-dialog"
    >
      <header class="conflict-header">
        <h2 id="spec-conflict-title" class="conflict-title">{$t('spec_conflict.title')}</h2>
        <button class="conflict-close" onclick={() => onClose?.()} aria-label={$t('spec_conflict.close')}>×</button>
      </header>

      <p class="conflict-explain">{$t('spec_conflict.explanation', { values: { path: conflict.spec_path ?? '' } })}</p>

      <SpecDiffView diff={conflict?.diff ?? []} />

      <footer class="conflict-actions">
        <Button variant="primary" onclick={() => onOverwrite?.()} data-testid="conflict-overwrite-btn">
          {$t('spec_conflict.overwrite')}
        </Button>
        <Button variant="secondary" onclick={() => onDiscard?.()} data-testid="conflict-discard-btn">
          {$t('spec_conflict.discard')}
        </Button>
        <Button variant="secondary" onclick={copyMine} data-testid="conflict-copy-btn">
          {$t('spec_conflict.copy')}
        </Button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .conflict-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
    padding: 1rem;
  }
  .conflict-dialog {
    background: var(--surface, #fff);
    color: var(--text, #111);
    border-radius: 8px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.35);
    max-width: 900px;
    width: 100%;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .conflict-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border, #e2e2e2);
  }
  .conflict-title {
    font-size: 1rem;
    font-weight: 600;
    margin: 0;
  }
  .conflict-close {
    background: none;
    border: none;
    font-size: 1.5rem;
    line-height: 1;
    cursor: pointer;
    color: var(--text-muted, #666);
  }
  .conflict-explain {
    padding: 0.75rem 1rem 0;
    margin: 0;
    color: var(--text-muted, #555);
    font-size: 0.85rem;
  }
  .conflict-actions {
    display: flex;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    border-top: 1px solid var(--border, #e2e2e2);
    justify-content: flex-end;
  }
</style>
