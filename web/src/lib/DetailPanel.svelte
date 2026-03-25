<script>
  import Tabs from './Tabs.svelte';
  import Button from './Button.svelte';

  /**
   * DetailPanel — slide-in panel from the right.
   *
   * Spec ref: ui-layout.md §2 (Split layout), §3 (Drill-Down pattern)
   *
   * Props:
   *   entity   — { type, id, data } | null
   *   expanded — bool, true when popped out to full-width
   *   onclose  — () => void
   *   onpopout — () => void
   */
  let {
    entity = null,
    expanded = $bindable(false),
    onclose = undefined,
    onpopout = undefined,
  } = $props();

  let activeTab = $state('info');
  let panelEl = $state(null);

  // Compute which tabs to show based on entity type.
  // Spec: ui-layout.md §2 "Detail panel tabs (contextual)"
  let tabs = $derived(computeTabs(entity));

  function computeTabs(ent) {
    if (!ent) return [];
    const type = ent.type;
    const data = ent.data ?? {};

    if (type === 'spec') {
      // Spec entities from the Specs view: richer tab set
      return [
        { id: 'content',     label: 'Content' },
        { id: 'edit',        label: 'Edit' },
        { id: 'progress',    label: 'Progress' },
        { id: 'links',       label: 'Links' },
        { id: 'history',     label: 'History' },
      ];
    }

    const result = [{ id: 'info', label: 'Info' }];

    if (type === 'mr') {
      result.push(
        { id: 'diff',        label: 'Diff' },
        { id: 'gates',       label: 'Gates' },
      );
      if (data.status === 'merged') {
        result.push({ id: 'attestation', label: 'Attestation' });
      }
      result.push({
        id: 'ask-why',
        label: 'Ask Why',
        disabled: !data.conversation_sha,
        title: data.conversation_sha ? undefined : 'Conversation unavailable',
      });
      return result;
    }

    if (type === 'agent') {
      result.push(
        { id: 'chat',    label: 'Chat' },
        { id: 'history', label: 'History' },
        { id: 'trace',   label: 'Trace' },
      );
      if (data.conversation_sha !== undefined) {
        result.push({
          id: 'ask-why',
          label: 'Ask Why',
          disabled: !data.conversation_sha,
          title: data.conversation_sha ? undefined : 'Conversation unavailable',
        });
      }
      return result;
    }

    if (type === 'node') {
      if (data.spec_path) result.push({ id: 'spec', label: 'Spec' });
      if (data.author_agent_id) result.push({ id: 'chat', label: 'Chat' });
      result.push({ id: 'history', label: 'History' });
      return result;
    }

    // Generic: info + optional extras
    if (data.spec_path) result.push({ id: 'spec', label: 'Spec' });
    if (data.author_agent_id) result.push({ id: 'chat', label: 'Chat' });
    if (data.has_history) result.push({ id: 'history', label: 'History' });
    return result;
  }

  // Reset active tab when entity changes, defaulting to the first tab.
  $effect(() => {
    if (entity) {
      const first = tabs[0];
      if (first) activeTab = first.id;
    }
  });

  // Keyboard: Esc closes the panel.
  function onkeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
    }
  }

  function close() {
    expanded = false;
    onclose?.();
  }

  function popout() {
    expanded = !expanded;
    onpopout?.();
    // Update URL to reflect expanded state (deep-linkable).
    if (entity) {
      const url = new URL(window.location.href);
      if (expanded) {
        url.searchParams.set('detail', `${entity.type}:${entity.id}`);
        url.searchParams.set('expanded', 'true');
      } else {
        url.searchParams.delete('expanded');
      }
      window.history.pushState({}, '', url.toString());
    }
  }

  // Focus management: when panel opens, move focus into it.
  $effect(() => {
    if (entity && panelEl) {
      const focusable = panelEl.querySelector(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      focusable?.focus();
    }
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside
  class="detail-panel"
  class:expanded
  class:open={!!entity}
  aria-label="Detail panel"
  onkeydown={onkeydown}
  bind:this={panelEl}
>
  {#if entity}
    <div class="panel-header">
      <div class="panel-entity">
        <span class="entity-type">{entity.type}</span>
        <span class="entity-id">{entity.data?.name ?? entity.id}</span>
      </div>
      <div class="panel-actions">
        <button
          class="panel-btn"
          onclick={popout}
          aria-label={expanded ? 'Collapse panel' : 'Pop out to full width'}
          title={expanded ? 'Collapse' : 'Pop Out'}
        >
          {#if expanded}
            <!-- Collapse icon -->
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
              <path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3"/>
            </svg>
          {:else}
            <!-- Pop out icon -->
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
              <polyline points="15 3 21 3 21 9"/><polyline points="9 21 3 21 3 15"/>
              <line x1="21" y1="3" x2="14" y2="10"/><line x1="3" y1="21" x2="10" y2="14"/>
            </svg>
          {/if}
          <span class="sr-only">{expanded ? 'Collapse' : 'Pop Out'}</span>
        </button>
        <button
          class="panel-btn panel-close"
          onclick={close}
          aria-label="Close detail panel"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
            <path d="M18 6L6 18M6 6l12 12"/>
          </svg>
        </button>
      </div>
    </div>

    <Tabs {tabs} bind:active={activeTab} panelId="detail-panel-content" />

    <div class="panel-content" id="detail-panel-content" role="tabpanel" aria-labelledby="tab-{activeTab}">
      {#if activeTab === 'info'}
        <div class="tab-pane">
          <dl class="entity-meta">
            <dt>Type</dt><dd>{entity.type}</dd>
            <dt>ID</dt><dd class="mono">{entity.id}</dd>
            {#if entity.data?.status}
              <dt>Status</dt><dd>{entity.data.status}</dd>
            {/if}
            {#if entity.data?.created_at}
              <dt>Created</dt><dd>{new Date(entity.data.created_at * 1000).toLocaleString()}</dd>
            {/if}
            {#if entity.data?.spec_path}
              <dt>Spec</dt><dd class="mono">{entity.data.spec_path}</dd>
            {/if}
          </dl>
        </div>

      {:else if activeTab === 'content'}
        <div class="tab-pane spec-content">
          <p class="placeholder-text">Spec content viewer — implemented by Specs view slice.</p>
        </div>

      {:else if activeTab === 'edit'}
        <div class="tab-pane">
          <p class="placeholder-text">Spec editor with LLM assist — implemented by Specs view slice.</p>
        </div>

      {:else if activeTab === 'progress'}
        <div class="tab-pane">
          <p class="placeholder-text">Task progress rollup — implemented by Specs view slice.</p>
        </div>

      {:else if activeTab === 'links'}
        <div class="tab-pane">
          <p class="placeholder-text">Spec link graph — implemented by Explorer/Specs slice.</p>
        </div>

      {:else if activeTab === 'spec'}
        <div class="tab-pane">
          <p class="placeholder-text">Spec viewer for {entity.data?.spec_path ?? 'this entity'}.</p>
        </div>

      {:else if activeTab === 'chat'}
        <div class="tab-pane">
          <p class="placeholder-text">Inline chat — implemented by InlineChat component.</p>
        </div>

      {:else if activeTab === 'history'}
        <div class="tab-pane">
          <p class="placeholder-text">Modification history timeline.</p>
        </div>

      {:else if activeTab === 'diff'}
        <div class="tab-pane">
          <p class="placeholder-text">Side-by-side code diff — implemented by MR slice.</p>
        </div>

      {:else if activeTab === 'gates'}
        <div class="tab-pane">
          <p class="placeholder-text">Gate execution results — implemented by MR slice.</p>
        </div>

      {:else if activeTab === 'attestation'}
        <div class="tab-pane">
          <p class="placeholder-text">Merge attestation bundle + conversation provenance.</p>
        </div>

      {:else if activeTab === 'trace'}
        <div class="tab-pane">
          <p class="placeholder-text">System trace timeline — implemented by MR/Agent slice.</p>
        </div>

      {:else if activeTab === 'ask-why'}
        <div class="tab-pane ask-why">
          {#if entity.data?.conversation_sha}
            <button
              class="start-interrogation"
              onclick={() => {/* Spawn interrogation agent — implemented by S2 slice */}}
            >
              Start interrogation
            </button>
            <p class="ask-why-hint">Spawns an interrogation agent to answer questions about this entity's decision history.</p>
          {:else}
            <p class="ask-why-unavailable">Conversation unavailable — no conversation SHA recorded for this entity.</p>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</aside>

<style>
  .detail-panel {
    width: 0;
    min-width: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
    transition: width 200ms ease-out, min-width 200ms ease-out;
    flex-shrink: 0;
  }

  .detail-panel.open {
    width: 40%;
    min-width: 320px;
  }

  .detail-panel.expanded {
    width: 100%;
    min-width: 0;
    border-left: none;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    flex-shrink: 0;
    gap: var(--space-2);
    min-height: 48px;
  }

  .panel-entity {
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow: hidden;
    min-width: 0;
  }

  .entity-type {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    font-weight: 600;
  }

  .entity-id {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .panel-actions {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex-shrink: 0;
  }

  .panel-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: none;
    border-radius: var(--radius);
    color: var(--color-text-muted);
    cursor: pointer;
    transition: color var(--transition-fast), background var(--transition-fast);
    padding: 0;
  }

  .panel-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-elevated);
  }

  .panel-close:hover {
    color: var(--color-danger);
  }

  .panel-content {
    flex: 1;
    overflow-y: auto;
    padding: var(--space-4);
  }

  .tab-pane {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  /* Entity metadata list */
  .entity-meta {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-2) var(--space-4);
    margin: 0;
    font-size: var(--text-sm);
  }

  .entity-meta dt {
    color: var(--color-text-muted);
    font-weight: 500;
    white-space: nowrap;
    padding: var(--space-1) 0;
  }

  .entity-meta dd {
    color: var(--color-text);
    margin: 0;
    padding: var(--space-1) 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .entity-meta dd.mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  /* Placeholder text for tabs implemented by other slices */
  .placeholder-text {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    font-style: italic;
    margin: 0;
    padding: var(--space-4) 0;
    text-align: center;
  }

  /* Ask Why tab */
  .ask-why {
    align-items: center;
    padding: var(--space-6) var(--space-4);
    text-align: center;
  }

  .start-interrogation {
    padding: var(--space-3) var(--space-6);
    background: var(--color-primary);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
    transition: background var(--transition-fast);
  }

  .start-interrogation:hover {
    background: var(--color-primary-hover);
  }

  .ask-why-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: var(--space-3) 0 0;
  }

  .ask-why-unavailable {
    font-size: var(--text-sm);
    color: var(--color-text-muted);
    margin: 0;
  }

  .mono {
    font-family: var(--font-mono);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border-width: 0;
  }
</style>
