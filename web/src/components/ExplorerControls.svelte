<script>
  import { getContext, onMount } from 'svelte';
  import { api } from '../lib/api.js';
  import { toast as showToast } from '../lib/toast.svelte.js';

  let {
    scope = 'workspace',       // 'tenant' | 'workspace' | 'repo'
    workspaceId = null,
    repoId = null,
    currentLens = $bindable('structural'),
    currentView = $bindable('boundary'),
    activeTab = $bindable('architecture'),
    onLensChange = null,
    onViewChange = null,
    onSearch = null,
    onFilterToggle = null,
    onPlaybackChange = null,
    showPlayback = false,
    traceTimeline = null,
  } = $props();

  const openDetailPanel = getContext('openDetailPanel');

  // Lens options
  const LENSES = [
    { id: 'structural', label: 'Structural' },
    { id: 'evaluative', label: 'Evaluative' },
    { id: 'observable', label: 'Observable', disabled: true },
  ];

  // Built-in view specs
  const BUILTIN_VIEWS = [
    { id: 'boundary', label: 'Boundary', spec: { name: 'Boundary View', layout: 'hierarchical', data: { depth: 1 }, encoding: { color: { field: 'node_type', scale: 'categorical' }, label: 'name' } } },
    { id: 'spec-realization', label: 'Spec Realization', spec: { name: 'Spec Realization', layout: 'side-by-side', data: { depth: 2 }, encoding: { border: { field: 'spec_confidence', scale: { high: '#22c55e', medium: '#eab308', low: '#f97316', none: '#ef4444' } }, label: 'qualified_name' } } },
    { id: 'change', label: 'Change', spec: { name: 'Change View', layout: 'timeline', data: { depth: 1 }, encoding: { size: { field: 'churn_count_30d', scale: 'linear', range: [24, 64] }, label: 'name' } } },
  ];

  // Saved views fetched from server
  let savedViews = $state([]);
  let savedViewsLoading = $state(false);

  // LLM Ask state
  let askQuery = $state('');
  let askLoading = $state(false);
  let askExplanation = $state('');
  let askError = $state('');
  let generatedViewSpec = $state(null);
  let askInputEl = $state(null);

  // Search state
  let searchQuery = $state('');
  let searchInputEl = $state(null);

  // Playback state (flow layout)
  let playbackSpeed = $state('1x');
  let currentTime = $state(0);
  const SPEEDS = ['0.25x', '0.5x', '1x', '2x', '4x'];

  onMount(() => {
    if (workspaceId) loadSavedViews();
    // Global '/' shortcut to focus search
    function onKeydown(e) {
      if (e.key === '/' && document.activeElement?.tagName !== 'INPUT' && document.activeElement?.tagName !== 'TEXTAREA') {
        e.preventDefault();
        e.stopPropagation();
        searchInputEl?.focus();
      }
    }
    document.addEventListener('keydown', onKeydown);
    return () => document.removeEventListener('keydown', onKeydown);
  });

  async function loadSavedViews() {
    if (!workspaceId) return;
    savedViewsLoading = true;
    try {
      savedViews = await api.explorerViews(workspaceId);
    } catch {
      savedViews = [];
    } finally {
      savedViewsLoading = false;
    }
  }

  function selectLens(lens) {
    if (lens.disabled) return;
    currentLens = lens.id;
    onLensChange?.(lens.id);
  }

  function selectView(viewId) {
    currentView = viewId;
    const builtin = BUILTIN_VIEWS.find(v => v.id === viewId);
    if (builtin) {
      onViewChange?.(builtin.spec);
      return;
    }
    const saved = savedViews.find(v => v.id === viewId);
    if (saved) {
      onViewChange?.(saved);
    }
  }

  function onSearchInput(e) {
    searchQuery = e.target.value;
    onSearch?.(searchQuery);
  }

  function clearSearch() {
    searchQuery = '';
    onSearch?.('');
    searchInputEl?.focus();
  }

  async function submitAsk() {
    if (!askQuery.trim() || !workspaceId || askLoading) return;
    askLoading = true;
    askExplanation = '';
    askError = '';
    generatedViewSpec = null;

    try {
      const res = await api.generateExplorerView(workspaceId, {
        question: askQuery.trim(),
        ...(repoId ? { repo_id: repoId } : {}),
      });

      if (!res.ok) {
        askError = `Request failed: ${res.status}`;
        askLoading = false;
        return;
      }

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });

        // SSE events are separated by blank lines (\n\n).
        // Split on double-newline to get complete event blocks.
        const eventBlocks = buffer.split('\n\n');
        // Keep the last (potentially incomplete) block in the buffer.
        buffer = eventBlocks.pop() ?? '';

        for (const block of eventBlocks) {
          if (!block.trim()) continue;
          const lines = block.split('\n');
          let eventType = 'message'; // default SSE event type
          let dataPayload = '';

          for (const line of lines) {
            if (line.startsWith('event: ')) {
              eventType = line.slice(7).trim();
            } else if (line.startsWith('data: ')) {
              dataPayload += (dataPayload ? '\n' : '') + line.slice(6);
            }
          }

          if (eventType === 'error') {
            // LLM connection failure — no fallback available
            askError = dataPayload || 'LLM connection failed';
          } else if (eventType === 'partial') {
            // Incremental explanation text chunk (not structured JSON)
            askExplanation += dataPayload;
          } else if (eventType === 'complete' || eventType === 'message') {
            // Final JSON response
            try {
              const parsed = JSON.parse(dataPayload);
              if (parsed.explanation) askExplanation = parsed.explanation;
              if ('view_spec' in parsed) {
                if (parsed.view_spec) {
                  generatedViewSpec = parsed.view_spec;
                  onViewChange?.(parsed.view_spec);
                } else {
                  // null view_spec = unanswerable question or invalid generated spec
                  askError = parsed.explanation || 'Could not generate a view for that question';
                }
              }
            } catch {
              // Malformed JSON in complete event — treat as plain text explanation
              if (dataPayload) askExplanation = dataPayload;
            }
          }
        }
      }
    } catch (e) {
      askError = 'LLM connection failed';
    } finally {
      askLoading = false;
    }
  }

  async function saveGeneratedView() {
    if (!generatedViewSpec || !workspaceId) return;
    try {
      const saved = await api.saveExplorerView(workspaceId, {
        name: generatedViewSpec.name || 'Generated view',
        ...generatedViewSpec,
      });
      savedViews = [...savedViews, saved];
      showToast('View saved', { type: 'success' });
      generatedViewSpec = null;
    } catch (e) {
      showToast('Failed to save view: ' + e.message, { type: 'error' });
    }
  }

  function onAskKeydown(e) {
    if (e.key === 'Enter') submitAsk();
  }

  function emitPlayback(cmd, value = undefined) {
    onPlaybackChange?.({ cmd, value });
  }

  function onScrubInput(e) {
    currentTime = Number(e.target.value);
    emitPlayback('scrub', currentTime);
  }

  function switchTab(tab) {
    activeTab = tab;
  }

  // Derived label for view selector
  let viewLabel = $derived.by(() => {
    const b = BUILTIN_VIEWS.find(v => v.id === currentView);
    if (b) return b.label;
    const s = savedViews.find(v => v.id === currentView);
    if (s) return s.name || 'Saved';
    return 'Custom';
  });
</script>

<div class="explorer-controls" role="toolbar" aria-label="Explorer controls">
  <!-- Repo scope: Architecture / Code tab switcher -->
  {#if scope === 'repo'}
    <div class="tab-switcher" role="tablist" aria-label="Explorer view mode">
      <button
        class="tab-btn {activeTab === 'architecture' ? 'active' : ''}"
        role="tab"
        aria-selected={activeTab === 'architecture'}
        onclick={() => switchTab('architecture')}
      >Architecture</button>
      <button
        class="tab-btn {activeTab === 'code' ? 'active' : ''}"
        role="tab"
        aria-selected={activeTab === 'code'}
        onclick={() => switchTab('code')}
      >Code</button>
    </div>
    <div class="divider" aria-hidden="true"></div>
  {/if}

  <!-- Canvas controls (hidden when Code tab active) -->
  {#if scope !== 'repo' || activeTab === 'architecture'}
    <!-- Lens selector -->
    <div class="control-group">
      <label class="control-label" for="lens-select">Lens</label>
      <div class="select-wrap">
        <select
          id="lens-select"
          class="ctrl-select"
          value={currentLens}
          onchange={(e) => selectLens(LENSES.find(l => l.id === e.target.value) ?? LENSES[0])}
          aria-label="Select visualization lens"
        >
          {#each LENSES as lens}
            <option value={lens.id} disabled={lens.disabled}>
              {lens.label}{lens.disabled ? ' (coming soon)' : ''}
            </option>
          {/each}
        </select>
        <span class="select-chevron" aria-hidden="true">▾</span>
      </div>
    </div>

    <!-- View selector -->
    <div class="control-group">
      <label class="control-label" for="view-select">View</label>
      <div class="select-wrap">
        <select
          id="view-select"
          class="ctrl-select"
          value={currentView}
          onchange={(e) => selectView(e.target.value)}
          aria-label="Select view preset"
        >
          <optgroup label="Built-in">
            {#each BUILTIN_VIEWS as v}
              <option value={v.id}>{v.label}</option>
            {/each}
          </optgroup>
          {#if savedViews.length > 0}
            <optgroup label="Saved">
              {#each savedViews as sv}
                <option value={sv.id}>{sv.name || 'Saved view'}</option>
              {/each}
            </optgroup>
          {/if}
        </select>
        <span class="select-chevron" aria-hidden="true">▾</span>
      </div>
    </div>

    <!-- Filter panel toggle -->
    <button
      class="ctrl-btn icon-btn"
      onclick={onFilterToggle}
      title="Toggle filter panel"
      aria-label="Toggle filter panel"
      type="button"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14" aria-hidden="true">
        <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>
      </svg>
      Filter
    </button>

    <!-- Search input -->
    <div class="search-wrap" title="Press / to focus">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="13" height="13" class="search-icon" aria-hidden="true">
        <circle cx="11" cy="11" r="8"/><path d="m21 21-4.35-4.35"/>
      </svg>
      <input
        type="search"
        class="ctrl-search"
        placeholder="Search nodes..."
        bind:this={searchInputEl}
        value={searchQuery}
        oninput={onSearchInput}
        onkeydown={(e) => {
          if (e.key === 'Escape') {
            e.stopPropagation();
            if (searchQuery) { clearSearch(); } else { searchInputEl?.blur(); }
          }
        }}
        aria-label="Search nodes"
      />
      {#if searchQuery}
        <button class="search-clear" onclick={clearSearch} aria-label="Clear search" type="button">✕</button>
      {/if}
      <kbd class="search-hint" aria-hidden="true">/</kbd>
    </div>

    <!-- LLM Ask input -->
    <div class="ask-wrap">
      <input
        type="text"
        class="ctrl-ask"
        placeholder="Ask: How does auth work?"
        bind:this={askInputEl}
        bind:value={askQuery}
        onkeydown={onAskKeydown}
        disabled={askLoading}
        aria-label="Ask LLM about architecture"
        aria-busy={askLoading}
      />
      <button
        class="ctrl-btn ask-btn"
        onclick={submitAsk}
        disabled={askLoading || !askQuery.trim()}
        type="button"
        aria-label="Generate view from question"
      >
        {#if askLoading}
          <span class="spinner" aria-hidden="true"></span>
        {:else}
          Ask
        {/if}
      </button>
    </div>
  {/if}

  <!-- Playback controls (flow layout only) -->
  {#if showPlayback && (scope !== 'repo' || activeTab === 'architecture')}
    <div class="divider" aria-hidden="true"></div>
    <div class="playback-controls" role="group" aria-label="Playback controls">
      <button class="ctrl-btn icon-btn" onclick={() => emitPlayback('play')} type="button" aria-label="Play">▶</button>
      <button class="ctrl-btn icon-btn" onclick={() => emitPlayback('pause')} type="button" aria-label="Pause">⏸</button>
      <button class="ctrl-btn icon-btn" onclick={() => emitPlayback('step')} type="button" aria-label="Step">⏭</button>

      <div class="control-group">
        <label class="control-label" for="speed-select">Speed</label>
        <div class="select-wrap">
          <select
            id="speed-select"
            class="ctrl-select ctrl-select-sm"
            value={playbackSpeed}
            onchange={(e) => { playbackSpeed = e.target.value; emitPlayback('speed', e.target.value); }}
            aria-label="Playback speed"
          >
            {#each SPEEDS as s}
              <option value={s}>{s}</option>
            {/each}
          </select>
          <span class="select-chevron" aria-hidden="true">▾</span>
        </div>
      </div>

      {#if traceTimeline}
        <input
          type="range"
          class="scrub-bar"
          min={traceTimeline.min}
          max={traceTimeline.max}
          value={currentTime || traceTimeline.current}
          oninput={onScrubInput}
          aria-label="Scrub timeline"
        />
      {/if}
    </div>
  {/if}

  <!-- Save generated view button -->
  {#if generatedViewSpec}
    <button class="ctrl-btn save-view-btn" onclick={saveGeneratedView} type="button">
      + Save View
    </button>
  {/if}
</div>

<!-- LLM explanation / error (below controls) -->
{#if askExplanation || askError}
  <div class="ask-feedback" role="status" aria-live="polite">
    {#if askError}
      <span class="ask-error">{askError}</span>
    {:else if askExplanation}
      <span class="ask-explanation">{askExplanation}</span>
    {/if}
  </div>
{/if}

<style>
  .explorer-controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface-elevated);
    border-top: 1px solid var(--color-border);
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .divider {
    width: 1px;
    height: 20px;
    background: var(--color-border-strong);
    flex-shrink: 0;
  }

  /* Tab switcher (Architecture / Code) */
  .tab-switcher {
    display: flex;
    gap: 2px;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    padding: 2px;
  }

  .tab-btn {
    padding: 3px var(--space-3);
    background: transparent;
    border: none;
    border-radius: calc(var(--radius) - 2px);
    color: var(--color-text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background var(--transition-fast), color var(--transition-fast);
  }

  .tab-btn.active {
    background: var(--color-primary);
    color: var(--color-text-inverse);
  }

  .tab-btn:not(.active):hover {
    background: var(--color-surface-hover);
    color: var(--color-text);
  }

  /* Control groups */
  .control-group {
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .control-label {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    white-space: nowrap;
  }

  .select-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }

  .ctrl-select {
    appearance: none;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    padding: 3px var(--space-5) 3px var(--space-2);
    cursor: pointer;
    outline: none;
    min-width: 110px;
  }

  .ctrl-select-sm {
    min-width: 70px;
  }

  .ctrl-select:focus {
    border-color: var(--color-primary);
  }

  .ctrl-select:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .ctrl-select option:disabled {
    color: var(--color-text-muted);
  }

  .select-chevron {
    position: absolute;
    right: var(--space-2);
    pointer-events: none;
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }

  /* Buttons */
  .ctrl-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-3);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    color: var(--color-text);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }

  .ctrl-btn:hover:not(:disabled) {
    background: var(--color-surface-hover);
    border-color: var(--color-primary);
  }

  .ctrl-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .icon-btn {
    padding: var(--space-1) var(--space-2);
  }

  /* Search */
  .search-wrap {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    padding: 3px var(--space-2);
  }

  .search-wrap:focus-within {
    border-color: var(--color-primary);
  }

  .search-icon {
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  .ctrl-search {
    background: transparent;
    border: none;
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    outline: none;
    width: 140px;
  }

  .ctrl-search::placeholder { color: var(--color-text-muted); }
  .ctrl-search::-webkit-search-cancel-button { display: none; }

  .search-clear {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: var(--text-xs);
    padding: 0 2px;
    line-height: 1;
    transition: color var(--transition-fast);
  }

  .search-clear:hover { color: var(--color-text); }

  .search-hint {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 2px var(--space-1);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }
  .search-wrap:focus-within .search-hint { display: none; }

  /* LLM Ask */
  .ask-wrap {
    display: flex;
    align-items: center;
    gap: 0;
    background: var(--color-surface);
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .ask-wrap:focus-within {
    border-color: var(--color-primary);
  }

  .ctrl-ask {
    background: transparent;
    border: none;
    color: var(--color-text);
    font-family: var(--font-body);
    font-size: var(--text-sm);
    outline: none;
    padding: var(--space-1) var(--space-2);
    width: 200px;
  }

  .ctrl-ask::placeholder { color: var(--color-text-muted); }
  .ctrl-ask:disabled { opacity: 0.6; cursor: not-allowed; }

  .ask-btn {
    border: none;
    border-left: 1px solid var(--color-border-strong);
    border-radius: 0;
    background: var(--color-surface-elevated);
    font-size: var(--text-xs);
    padding: var(--space-1) var(--space-3);
  }

  /* Save view button */
  .save-view-btn {
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 40%, transparent);
    color: var(--color-success);
  }

  .save-view-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-success) 20%, transparent);
    border-color: var(--color-success);
  }

  /* Playback controls */
  .playback-controls {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex-wrap: wrap;
  }

  .scrub-bar {
    width: 120px;
    accent-color: var(--color-primary);
  }

  /* LLM feedback */
  .ask-feedback {
    padding: var(--space-2) var(--space-4);
    background: var(--color-surface);
    border-top: 1px solid var(--color-border);
    font-size: var(--text-sm);
    flex-shrink: 0;
  }

  .ask-explanation {
    color: var(--color-text-secondary);
    font-style: italic;
  }

  .ask-error {
    color: var(--color-danger);
  }

  .tab-btn:focus-visible,
  .ctrl-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  /* Spinner */
  @keyframes spin { to { transform: rotate(360deg); } }

  .spinner {
    display: inline-block;
    width: 10px;
    height: 10px;
    border: 2px solid var(--color-border-strong);
    border-top-color: var(--color-primary);
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  @media (prefers-reduced-motion: reduce) {
    .spinner { animation: none; opacity: 0.6; }
  }
</style>
