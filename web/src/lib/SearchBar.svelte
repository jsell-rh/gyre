<script>
  let { onnavigate = undefined } = $props();

  let open = $state(false);
  let query = $state('');
  let inputEl = $state(null);

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

  let results = $derived(
    query.trim().length < 1
      ? SHORTCUTS
      : SHORTCUTS.filter(s => s.label.toLowerCase().includes(query.toLowerCase()))
  );

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

  function navigate(item) {
    open = false;
    onnavigate?.(item.view);
  }

  function onkeydown(e) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selected = (selected + 1) % results.length;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selected = (selected - 1 + results.length) % results.length;
    } else if (e.key === 'Enter' && results[selected]) {
      navigate(results[selected]);
    }
  }
</script>

{#if open}
  <div class="search-backdrop" onclick={() => (open = false)} aria-hidden="true"></div>
  <div class="search-dialog" role="dialog" aria-label="Quick navigation">
    <div class="search-input-wrap">
      <svg class="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
        <circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/>
      </svg>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        bind:this={inputEl}
        bind:value={query}
        onkeydown={onkeydown}
        type="text"
        placeholder="Search or navigate..."
        class="search-input"
        autocomplete="off"
        spellcheck="false"
      />
      <kbd class="search-esc">Esc</kbd>
    </div>

    {#if results.length > 0}
      <ul class="search-results" role="listbox">
        {#each results as item, i}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            role="option"
            aria-selected={selected === i}
            class="search-result"
            class:active={selected === i}
            onclick={() => navigate(item)}
            onmouseenter={() => (selected = i)}
          >
            <span class="result-icon">{item.icon}</span>
            <span class="result-label">{item.label}</span>
          </li>
        {/each}
      </ul>
    {:else}
      <div class="search-empty">No results for "{query}"</div>
    {/if}

    <div class="search-footer">
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
    background: rgba(0, 0, 0, 0.5);
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

  .result-label {
    font-size: var(--text-sm);
    font-weight: 500;
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
