<script>
  /**
   * RepoMode — repo view with horizontal tab bar (§3 of ui-navigation.md)
   *
   * Tab bar is a placeholder — Slice 3 will wire actual tab content.
   * Reuses existing components for content areas where available.
   */
  import ExplorerView from './ExplorerView.svelte';
  import SpecDashboard from './SpecDashboard.svelte';
  import Inbox from './Inbox.svelte';
  import ExplorerCodeTab from './ExplorerCodeTab.svelte';

  let {
    workspace = null,
    repo = null,
    activeTab = 'specs',
    onTabChange = undefined,
  } = $props();

  const TABS = [
    { id: 'specs',        label: 'Specs' },
    { id: 'architecture', label: 'Architecture' },
    { id: 'decisions',    label: 'Decisions' },
    { id: 'code',         label: 'Code' },
    { id: 'settings',     label: '⚙', title: 'Settings' },
  ];
</script>

<div class="repo-mode" data-testid="repo-mode">
  <!-- Tab bar -->
  <nav class="tab-bar" aria-label="Repo navigation" data-testid="repo-tab-bar">
    {#each TABS as tab}
      <button
        class="tab-btn"
        class:active={activeTab === tab.id}
        onclick={() => onTabChange?.(tab.id)}
        aria-current={activeTab === tab.id ? 'page' : undefined}
        title={tab.title ?? tab.label}
      >
        {tab.label}
      </button>
    {/each}
  </nav>

  <!-- Tab content -->
  <div class="tab-content">
    {#if activeTab === 'specs'}
      <SpecDashboard
        workspaceId={workspace?.id ?? null}
        repoId={repo?.id ?? null}
        scope="repo"
      />
    {:else if activeTab === 'architecture'}
      <ExplorerView
        scope={{ type: 'repo', workspaceId: workspace?.id, repoId: repo?.id }}
        workspaceName={workspace?.name ?? null}
      />
    {:else if activeTab === 'decisions'}
      <Inbox workspaceId={workspace?.id} scope="repo" />
    {:else if activeTab === 'code'}
      {#if repo?.id}
        <ExplorerCodeTab repoId={repo.id} />
      {:else}
        <div class="tab-placeholder">
          <p>No repo selected.</p>
        </div>
      {/if}
    {:else if activeTab === 'settings'}
      <div class="tab-placeholder" data-testid="settings-placeholder">
        <p>Repo settings coming in Slice 4.</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .repo-mode {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  /* ── Tab bar ────────────────────────────────────────────────────────── */
  .tab-bar {
    display: flex;
    align-items: center;
    gap: 0;
    padding: 0 var(--space-4);
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    overflow-x: auto;
  }

  .tab-btn {
    padding: var(--space-3) var(--space-4);
    background: transparent;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--color-text-secondary);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
    transition: color var(--transition-fast), border-color var(--transition-fast);
    margin-bottom: -1px;
  }

  .tab-btn:hover {
    color: var(--color-text);
  }

  .tab-btn.active {
    color: var(--color-text);
    border-bottom-color: var(--color-primary);
    font-weight: 500;
  }

  .tab-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: -2px;
  }

  /* ── Tab content ────────────────────────────────────────────────────── */
  .tab-content {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .tab-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    padding: var(--space-8);
    color: var(--color-text-muted);
    font-size: var(--text-sm);
    font-style: italic;
  }

  .tab-placeholder p {
    margin: 0;
  }

  @media (max-width: 768px) {
    .tab-bar {
      padding: 0 var(--space-2);
    }

    .tab-btn {
      padding: var(--space-3) var(--space-3);
      font-size: var(--text-xs);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .tab-btn {
      transition: none;
    }
  }
</style>
