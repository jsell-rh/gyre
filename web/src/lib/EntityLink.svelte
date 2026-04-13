<script>
  /**
   * EntityLink — clickable entity reference with human-friendly name.
   *
   * Renders entity name from shared cache, full ID as tooltip.
   * Click navigates to entity detail view.
   */
  import { getContext } from 'svelte';
  import { entityName, shortId, formatSha } from './entityNames.svelte.js';

  const goToEntityDetail = getContext('goToEntityDetail') ?? null;

  let {
    type = 'agent',
    id = '',
    data = undefined,
    showType = false,
    sha = false,
    onclick = undefined,
  } = $props();

  const TYPE_LABELS = {
    agent: 'Agent',
    task: 'Task',
    mr: 'MR',
    spec: 'Spec',
    repo: 'Repo',
    workspace: 'Workspace',
  };

  let displayName = $derived.by(() => {
    if (!id) return '';
    if (sha) return formatSha(id);
    if (type === 'spec') return id.split('/').pop()?.replace(/\.md$/, '') ?? id;
    return entityName(type, id);
  });

  let copyFeedback = $state(false);

  function handleClick(e) {
    e.stopPropagation();
    if (onclick) {
      onclick(e);
      return;
    }
    goToEntityDetail?.(type, id, data ?? {});
  }

  function handleCopy(e) {
    e.stopPropagation();
    e.preventDefault();
    navigator.clipboard.writeText(id);
    copyFeedback = true;
    setTimeout(() => { copyFeedback = false; }, 1200);
  }
</script>

{#if id}
  <span class="entity-link-wrapper">
    <button
      class="entity-link"
      onclick={handleClick}
      title={id}
      aria-label="{TYPE_LABELS[type] ?? type}: {displayName}"
    >
      {#if showType}<span class="entity-link-type">{TYPE_LABELS[type] ?? type}</span>{/if}
      <span class="entity-link-name">{displayName}</span>
    </button>
    <button
      class="entity-copy-btn"
      onclick={handleCopy}
      title="Copy ID to clipboard"
      aria-label="Copy {TYPE_LABELS[type] ?? type} ID"
    >
      {#if copyFeedback}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10"><polyline points="20 6 9 17 4 12"/></svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 01-2-2V4a2 2 0 012-2h9a2 2 0 012 2v1"/></svg>
      {/if}
    </button>
  </span>
{/if}

<style>
  .entity-link {
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--color-link, var(--color-primary));
    text-decoration: none;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 160px;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    vertical-align: middle;
    text-align: left;
    line-height: 1.4;
  }

  .entity-link:hover {
    text-decoration: underline;
    color: var(--color-primary);
  }

  .entity-link:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 1px;
    border-radius: var(--radius-sm);
  }

  .entity-link-type {
    font-size: 0.85em;
    color: var(--color-text-muted);
    font-family: var(--font-body);
  }

  .entity-link-name {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .entity-link-wrapper {
    display: inline-flex;
    align-items: center;
    gap: 0;
    max-width: 180px;
    vertical-align: middle;
  }

  .entity-copy-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    padding: 1px 2px;
    cursor: pointer;
    color: var(--color-text-muted);
    opacity: 0;
    transition: opacity var(--transition-fast);
    flex-shrink: 0;
  }

  .entity-link-wrapper:hover .entity-copy-btn {
    opacity: 0.7;
  }

  .entity-copy-btn:hover {
    opacity: 1 !important;
    color: var(--color-primary);
  }
</style>
