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

  function handleClick(e) {
    e.stopPropagation();
    if (onclick) {
      onclick(e);
      return;
    }
    goToEntityDetail?.(type, id, data ?? {});
  }
</script>

{#if id}
  <button
    class="entity-link"
    onclick={handleClick}
    title={id}
    aria-label="{TYPE_LABELS[type] ?? type}: {displayName}"
  >
    {#if showType}<span class="entity-link-type">{TYPE_LABELS[type] ?? type}</span>{/if}
    <span class="entity-link-name">{displayName}</span>
  </button>
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
</style>
