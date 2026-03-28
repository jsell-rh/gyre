<script>
  /**
   * WorkspaceHome — workspace dashboard (§2 of ui-navigation.md)
   *
   * Sections: Decisions, Repos, Briefing, Specs, Agent Rules.
   * All sections are skeletons — other slices will fill them in.
   */
  let {
    workspace = null,
    onSelectRepo = undefined,
    decisionsCount = 0,
  } = $props();
</script>

<div class="workspace-home" data-testid="workspace-home">
  {#if !workspace}
    <!-- No workspace selected — prompt user to select one -->
    <div class="no-workspace">
      <div class="no-workspace-icon" aria-hidden="true">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" width="48" height="48">
          <path d="M3 9l9-7 9 7v11a2 2 0 01-2 2H5a2 2 0 01-2-2z"/>
          <polyline points="9 22 9 12 15 12 15 22"/>
        </svg>
      </div>
      <h2 class="no-workspace-title">Select a workspace</h2>
      <p class="no-workspace-desc">Choose a workspace from the selector above to get started.</p>
    </div>
  {:else}
    <div class="sections">

      <!-- ── Decisions ─────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-decisions" data-testid="section-decisions">
        <div class="section-header">
          <h2 class="section-title" id="section-decisions">
            Decisions
            {#if decisionsCount > 0}
              <span class="section-badge" aria-label="{decisionsCount} decisions">{decisionsCount}</span>
            {/if}
          </h2>
        </div>
        <div class="section-body section-placeholder">
          {#if decisionsCount === 0}
            <p class="placeholder-text">No decisions needed — system is running autonomously.</p>
          {:else}
            <p class="placeholder-text">{decisionsCount} item{decisionsCount !== 1 ? 's' : ''} require your attention. (Decisions section coming soon.)</p>
          {/if}
        </div>
      </section>

      <!-- ── Repos ─────────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-repos" data-testid="section-repos">
        <div class="section-header">
          <h2 class="section-title" id="section-repos">Repos</h2>
        </div>
        <div class="section-body section-placeholder">
          <p class="placeholder-text">Repository list coming soon. Click a repo to enter repo mode.</p>
          <div class="placeholder-actions">
            <button class="placeholder-btn" disabled aria-disabled="true">+ New Repo</button>
            <button class="placeholder-btn" disabled aria-disabled="true">Import</button>
          </div>
        </div>
      </section>

      <!-- ── Briefing ──────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-briefing" data-testid="section-briefing">
        <div class="section-header">
          <h2 class="section-title" id="section-briefing">Briefing</h2>
        </div>
        <div class="section-body section-placeholder">
          <p class="placeholder-text">LLM-synthesized activity summary coming soon.</p>
        </div>
      </section>

      <!-- ── Specs ─────────────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-specs" data-testid="section-specs">
        <div class="section-header">
          <h2 class="section-title" id="section-specs">Specs</h2>
        </div>
        <div class="section-body section-placeholder">
          <p class="placeholder-text">Cross-repo spec overview coming soon.</p>
        </div>
      </section>

      <!-- ── Agent Rules ───────────────────────────────────────────────── -->
      <section class="home-section" aria-labelledby="section-agent-rules" data-testid="section-agent-rules">
        <div class="section-header">
          <h2 class="section-title" id="section-agent-rules">Agent Rules</h2>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <a class="section-action" href="/workspaces/{workspace.slug ?? workspace.id}/agent-rules"
             onclick={(e) => { e.preventDefault(); window.history.pushState({ mode: 'workspace_home', wsId: workspace.id, repoName: null, repoTab: 'specs' }, '', `/workspaces/${encodeURIComponent(workspace.slug ?? workspace.id)}/agent-rules`); }}
          >Manage rules</a>
        </div>
        <div class="section-body section-placeholder">
          <p class="placeholder-text">Meta-spec cascade summary coming soon.</p>
        </div>
      </section>

    </div>
  {/if}
</div>

<style>
  .workspace-home {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-6) var(--space-8);
    max-width: 860px;
    margin: 0 auto;
    width: 100%;
  }

  /* ── No workspace selected ──────────────────────────────────────────── */
  .no-workspace {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    padding: var(--space-16) var(--space-8);
    text-align: center;
    color: var(--color-text-muted);
  }

  .no-workspace-icon {
    opacity: 0.3;
  }

  .no-workspace-title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 600;
    color: var(--color-text-secondary);
    margin: 0;
  }

  .no-workspace-desc {
    font-size: var(--text-sm);
    margin: 0;
  }

  /* ── Sections layout ────────────────────────────────────────────────── */
  .sections {
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .home-section {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    overflow: hidden;
  }

  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .section-title {
    font-family: var(--font-display);
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    margin: 0;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .section-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 var(--space-1);
    background: var(--color-danger);
    color: var(--color-text-inverse);
    border-radius: var(--radius-full);
    font-size: var(--text-xs);
    font-weight: 700;
  }

  .section-action {
    font-size: var(--text-xs);
    color: var(--color-primary);
    text-decoration: none;
  }

  .section-action:hover {
    text-decoration: underline;
  }

  .section-body {
    padding: var(--space-4);
  }

  /* ── Placeholder state ──────────────────────────────────────────────── */
  .section-placeholder {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .placeholder-text {
    margin: 0;
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
  }

  .placeholder-actions {
    display: flex;
    gap: var(--space-2);
  }

  .placeholder-btn {
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text-muted);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    cursor: not-allowed;
    opacity: 0.6;
  }

  @media (max-width: 768px) {
    .workspace-home {
      padding: var(--space-4);
    }
  }
</style>
