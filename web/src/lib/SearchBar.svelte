<script>
  import { api } from './api.js';

  let { open = $bindable(false), onnavigate = undefined } = $props();
  let query = $state('');
  let inputEl = $state(null);
  let apiResults = $state([]);
  let searching = $state(false);
  let searchTimer = null;

  const ENTITY_ICONS = { task: 'T', agent: 'G', mr: 'M', spec: 'S' };

  const SHORTCUTS = [
    { label: 'Dashboard', view: 'dashboard', icon: 'H' },
    { label: 'Activity Feed', view: 'activity', icon: 'A' },
    { label: 'Agents', view: 'agents', icon: 'G' },
    { label: 'Task Board', view: 'tasks', icon: 'T' },
    { label: 'Projects', view: 'projects', icon: 'P' },
    { label: 'Merge Queue', view: 'merge-queue', icon: 'Q' },
    { label: 'Analytics', view: 'analytics', icon: 'N' },
    { label: 'Admin Panel', view: 'admin', icon: 'D' },
  ];

  // Combined results: API entity hits + nav shortcuts.
  let results = $derived(
    query.trim().length < 1
      ? SHORTCUTS
      : [
          ...apiResults.map(r => ({
            label: r.title,
            snippet: r.snippet,
            icon: ENTITY_ICONS[r.entity_type] ?? '?',
            entityType: r.entity_type,
            entityId: r.entity_id,
            view: entityView(r.entity_type),
          })),
          ...SHORTCUTS.filter(s => s.label.toLowerCase().includes(query.toLowerCase())),
        ]
  );

  function entityView(type) {
    const map = { task: 'tasks', agent: 'agents', mr: 'merge-requests', spec: 'specs' };
    return map[type] ?? 'dashboard';
  }

  // Debounced API search: fires 300ms after typing stops.
  $effect(() => {
    const q = query.trim();
    if (q.length < 2) {
      apiResults = [];
      return;
    }
    clearTimeout(searchTimer);
    searchTimer = setTimeout(async () => {
      searching = true;
      try {
        const data = await api.search({ q, limit: 8 });
        apiResults = data?.results ?? [];
      } catch {
        apiResults = [];
      } finally {
        searching = false;
      }
    }, 300);
    return () => clearTimeout(searchTimer);
  });

  let selected = $state(0);

  $effect(() => {
    function onkey(e) {
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        open = true;
        selected = 0;
        query = '';
      }
      if (e.key === 'Escape' && open) {
        open = false;
      }
    }
    window.addEventListener('keydown', onkey);
    return () => window.removeEventListener('keydown', onkey);
  });

  $effect(() => {
    if (open && inputEl) {
      setTimeout(() => inputEl?.focus(), 10);
    }
  });

  // Reset selection when results change.
  $effect(() => {
    results; // track
    selected = 0;
  });

  function navigate(item) {
    open = false;
    onnavigate?.(item.view);
  }

  function onkeydown(e) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selected = (selected + 1) % Math.max(results.length, 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selected = (selected - 1 + Math.max(results.length, 1)) % Math.max(results.length, 1);
    } else if (e.key === 'Enter' && results[selected]) {
      navigate(results[selected]);
    }
  }
</script>

{#if open}
  <div class="search-backdrop" onclick={() => (open = false)} aria-hidden="true"></div>
  <div
    class="search-dialog"
    role="dialog"
    aria-label="Quick navigation"
    aria-modal="true"
  >
    <div class="search-input-wrap">
      <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16" aria-hidden="true">
        <circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/>
      </svg>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        bind:this={inputEl}
        bind:value={query}
        onkeydown={onkeydown}
        type="text"
        placeholder="Search tasks, agents, specs, MRs..."
        class="search-input"
        autocomplete="off"
        spellcheck="false"
        aria-label="Search or navigate"
        aria-autocomplete="list"
        aria-controls="search-listbox"
        aria-activedescendant={results.length > 0 ? `search-option-${selected}` : undefined}
        role="combobox"
        aria-expanded={results.length > 0}
        aria-haspopup="listbox"
      />
      {#if searching}
        <span class="search-spinner" aria-label="Searching...">⟳</span>
      {:else}
        <kbd class="search-esc" aria-hidden="true">Esc</kbd>
      {/if}
    </div>

    {#if results.length > 0}
      <ul class="search-results" role="listbox" id="search-listbox" aria-label="Search results">
        {#each results as item, i}
          <li
            role="option"
            id="search-option-{i}"
            aria-selected={selected === i}
            class="search-result"
            class:active={selected === i}
            onclick={() => navigate(item)}
            onkeydown={(e) => e.key === 'Enter' && navigate(item)}
            onmouseenter={() => (selected = i)}
            tabindex="-1"
          >
            <span class="result-icon" aria-hidden="true">{item.icon}</span>
            <span class="result-content">
              <span class="result-label">{item.label}</span>
              {#if item.entityType}
                <span class="result-type">{item.entityType}</span>
              {/if}
              {#if item.snippet}
                <span class="result-snippet">{item.snippet}</span>
              {/if}
            </span>
          </li>
        {/each}
      </ul>
    {:else if query.trim().length >= 2 && !searching}
      <div class="search-empty" role="status">No results for "{query}"</div>
    {/if}

    <div class="search-footer" aria-hidden="true">
      <span><kbd>↑↓</kbd> navigate</span>
      <span><kbd>↵</kbd> select</span>
      <span><kbd>Esc</kbd> close</span>
    </div>
  </div>
{/if}

<style>
  .search-backdrop {
    position: fixed;
    inset: 0;
    z-index: 900;
    background: color-mix(in srgb, black 50%, transparent);
    backdrop-filter: blur(2px);
  }

  .search-dialog {
    position: fixed;
    top: 20%;
    left: 50%;
    transform: translateX(-50%);
    z-index: 901;
    width: min(560px, calc(100vw - 2rem));
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    animation: search-in 120ms ease;
    overflow: hidden;
  }

  @keyframes search-in {
    from { opacity: 0; transform: translateX(-50%) translateY(-12px); }
    to   { opacity: 1; transform: translateX(-50%) translateY(0); }
  }

  .search-input-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-4);
    border-bottom: 1px solid var(--color-border);
  }

  .search-icon {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    font-family: var(--font-body);
    font-size: var(--text-base);
    color: var(--color-text);
  }

  .search-input::placeholder { color: var(--color-text-muted); }

  .search-esc {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    padding: 2px 6px;
    font-family: var(--font-mono);
  }

  .search-results {
    list-style: none;
    max-height: 340px;
    overflow-y: auto;
    padding: var(--space-1);
  }

  .search-result {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
    cursor: pointer;
    transition: background var(--transition-fast);
    color: var(--color-text-secondary);
  }

  .search-result.active,
  .search-result:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .result-icon {
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--color-border);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    font-weight: 600;
    color: var(--color-text-secondary);
    flex-shrink: 0;
  }

  .result-content {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .result-label {
    font-size: var(--text-sm);
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .result-type {
    font-size: var(--text-xs);
    color: var(--color-primary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-family: var(--font-mono);
  }

  .result-snippet {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .search-spinner {
    color: var(--color-text-muted);
    font-size: var(--text-base);
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .search-empty {
    padding: var(--space-8) var(--space-4);
    text-align: center;
    color: var(--color-text-muted);
    font-size: var(--text-sm);
  }

  .search-footer {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-2) var(--space-4);
    border-top: 1px solid var(--color-border);
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  kbd {
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-sm);
    padding: 1px 4px;
    font-family: var(--font-mono);
    font-size: 0.7rem;
    color: var(--color-text-secondary);
  }
</style>
