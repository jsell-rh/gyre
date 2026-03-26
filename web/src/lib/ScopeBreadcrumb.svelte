<script>
  /**
   * ScopeBreadcrumb — topbar scope indicator with workspace dropdown.
   *
   * Spec ref: ui-layout.md §1 (Application Shell, Topbar)
   *           ui-layout.md §3 (Scope Transitions — 150ms opacity cross-fade)
   *           HSI §1 (Navigation Model, Scope Indicator)
   *
   * Props:
   *   tenant      — { id, name } | null
   *   workspace   — { id, name } | null
   *   repo        — { id, name } | null
   *   workspaces  — Array<{ id, name }> — member workspaces for dropdown
   *   onnavigate  — (view: string, ctx: object) => void
   *   class       — additional CSS classes
   */
  let {
    tenant = null,
    workspace = null,
    repo = null,
    workspaces = [],
    onnavigate = undefined,
    class: extraClass = '',
  } = $props();

  import { tick } from 'svelte';

  let dropdownOpen = $state(false);
  let dropdownEl = $state(null);
  let dropdownListEl = $state(null);

  function clickTenant() {
    onnavigate?.('explorer', { scope: 'tenant' });
    dropdownOpen = false;
  }

  function clickWorkspace() {
    if (workspaces.length > 1) {
      dropdownOpen = !dropdownOpen;
    } else {
      onnavigate?.('explorer', { scope: 'workspace', workspace });
    }
  }

  function selectWorkspace(ws) {
    dropdownOpen = false;
    onnavigate?.('explorer', { scope: 'workspace', workspace: ws });
  }

  function clickRepo() {
    onnavigate?.('explorer', { scope: 'repo', repo });
  }

  // Focus active option when dropdown opens.
  $effect(() => {
    if (dropdownOpen && dropdownListEl) {
      tick().then(() => {
        const active = dropdownListEl.querySelector('.ws-option.active')
          ?? dropdownListEl.querySelector('.ws-option');
        active?.focus();
      });
    }
  });

  function onDropdownKeydown(e) {
    const options = Array.from(dropdownListEl?.querySelectorAll('.ws-option') ?? []);
    const current = options.indexOf(document.activeElement);
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      options[(current + 1) % options.length]?.focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      options[(current - 1 + options.length) % options.length]?.focus();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      dropdownOpen = false;
      dropdownEl?.querySelector('.workspace-crumb')?.focus();
    } else if (e.key === 'Home') {
      e.preventDefault();
      options[0]?.focus();
    } else if (e.key === 'End') {
      e.preventDefault();
      options[options.length - 1]?.focus();
    }
  }

  // Close dropdown when clicking outside.
  $effect(() => {
    if (!dropdownOpen) return;
    function handleClick(e) {
      if (dropdownEl && !dropdownEl.contains(e.target)) {
        dropdownOpen = false;
      }
    }
    document.addEventListener('click', handleClick, true);
    return () => document.removeEventListener('click', handleClick, true);
  });
</script>

<nav class="scope-breadcrumb {extraClass}" aria-label="Scope">
  {#if tenant}
    <button class="crumb tenant-crumb" onclick={clickTenant}>
      {tenant.name}
    </button>
    <span class="sep" aria-hidden="true">›</span>
  {/if}

  {#if workspace}
    <div class="crumb-wrapper" bind:this={dropdownEl}>
      <button
        class="crumb workspace-crumb"
        class:has-dropdown={workspaces.length > 1}
        onclick={clickWorkspace}
        aria-haspopup={workspaces.length > 1 ? 'listbox' : undefined}
        aria-expanded={workspaces.length > 1 ? dropdownOpen : undefined}
      >
        {workspace.name}
        {#if workspaces.length > 1}
          <svg
            class="dropdown-caret"
            class:open={dropdownOpen}
            viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            width="10" height="10" aria-hidden="true"
          >
            <polyline points="6 9 12 15 18 9"/>
          </svg>
        {/if}
      </button>

      {#if dropdownOpen && workspaces.length > 1}
        <ul class="ws-dropdown" role="listbox" aria-label="Select workspace" bind:this={dropdownListEl} onkeydown={onDropdownKeydown}>
          {#each workspaces as ws}
            <li role="presentation">
              <button
                class="ws-option"
                class:active={ws.id === workspace?.id}
                role="option"
                aria-selected={ws.id === workspace?.id}
                onclick={() => selectWorkspace(ws)}
              >
                {ws.name}
                {#if ws.id === workspace?.id}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" width="12" height="12" aria-hidden="true">
                    <polyline points="20 6 9 17 4 12"/>
                  </svg>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if repo}
      <span class="sep" aria-hidden="true">›</span>
    {/if}
  {/if}

  {#if repo}
    <button class="crumb repo-crumb" onclick={clickRepo}>
      {repo.name}
    </button>
  {/if}
</nav>

<style>
  .scope-breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    font-size: var(--text-xs);
    position: relative;
  }

  .sep {
    color: var(--color-text-muted);
    user-select: none;
    font-size: var(--text-xs);
  }

  .crumb {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: transparent;
    border: none;
    padding: 0;
    cursor: pointer;
    font-family: var(--font-body);
    font-size: var(--text-xs);
    transition: color 150ms ease-out, opacity 150ms ease-out;
    white-space: nowrap;
    border-radius: var(--radius-sm);
  }

  .crumb:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }

  .ws-option:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: -2px;
  }

  .tenant-crumb {
    color: var(--color-text-muted);
  }
  .tenant-crumb:hover { color: var(--color-text-secondary); }

  .workspace-crumb {
    color: var(--color-text-secondary);
    font-weight: 500;
  }
  .workspace-crumb:hover { color: var(--color-text); }

  .workspace-crumb.has-dropdown:hover {
    color: var(--color-link);
  }

  .repo-crumb {
    color: var(--color-text-secondary);
    font-weight: 500;
  }
  .repo-crumb:hover { color: var(--color-text); }

  .dropdown-caret {
    transition: transform 150ms ease-out;
  }
  .dropdown-caret.open {
    transform: rotate(180deg);
  }

  .crumb-wrapper {
    position: relative;
    display: flex;
    align-items: center;
  }

  .ws-dropdown {
    position: absolute;
    top: calc(100% + var(--space-2));
    left: 0;
    z-index: 200;
    list-style: none;
    margin: 0;
    padding: var(--space-1);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg);
    min-width: 180px;
    max-height: 240px;
    overflow-y: auto;
  }

  .ws-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: var(--space-2) var(--space-3);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast);
    text-align: left;
  }

  .ws-option:hover {
    background: var(--color-surface-elevated);
    color: var(--color-text);
  }

  .ws-option.active {
    color: var(--color-text);
    font-weight: 500;
  }
</style>
