<script>
  /**
   * DiffView — custom Svelte component for the `diff` layout
   * (ui-layout.md §10 Layout Engines: `diff` → "Custom Svelte component:
   * added/modified/removed node cards").
   *
   * Computes the structural diff of the knowledge graph between a baseline
   * point in time and a comparison point, evaluated against real node state:
   *
   *   added    — nodes whose first_seen_at is after the baseline
   *   modified — nodes whose last_modified_at falls in the (baseline, compare]
   *              window (and were not added in it)
   *   removed  — soft-deleted nodes (deleted_at) deleted after the baseline
   *
   * Baseline/compare points come from the repo's architectural timeline
   * deltas when available (repoId set), otherwise the window defaults to the
   * last 7 days. Cards are interactive elements emitting standardized
   * ViewEvents.
   */
  import { api } from './api.js';
  import Badge from './Badge.svelte';
  import { dispatchViewEvent, viewEventFromDom } from './viewEvents.js';

  let {
    nodes = [],
    edges = [],
    repoId = '',
    onSelectNode = undefined,
  } = $props();

  const DAY = 86400;
  let deltas = $state([]);
  let baselineIndex = $state(0); // index into sorted-desc delta list
  let loading = $state(false);
  let error = $state(null);

  // Load the timeline for baseline/compare reference points.
  $effect(() => {
    const rid = repoId;
    if (!rid) {
      deltas = [];
      return;
    }
    let cancelled = false;
    loading = true;
    error = null;
    api
      .repoGraphTimeline(rid)
      .then((list) => {
        if (cancelled) return;
        deltas = (Array.isArray(list) ? list : [])
          .slice()
          .sort((a, b) => b.timestamp - a.timestamp);
      })
      .catch((e) => {
        if (!cancelled) error = e?.message ?? String(e);
      })
      .finally(() => {
        if (!cancelled) loading = false;
      });
    return () => {
      cancelled = true;
    };
  });

  let now = Math.floor(Date.now() / 1000);
  // Compare point: the most recent delta (or now). Baseline: the delta the
  // user picked (default: second-most-recent — "what changed in the last delta").
  let compareTs = $derived(deltas.length > 0 ? deltas[0].timestamp : now);
  let compareLabel = $derived(new Date(compareTs * 1000).toLocaleDateString());
  let baselineTs = $derived.by(() => {
    if (deltas.length > 1 && baselineIndex >= 0 && baselineIndex < deltas.length - 1) {
      return deltas[baselineIndex + 1].timestamp;
    }
    if (deltas.length === 1) return deltas[0].timestamp - DAY;
    return now - 7 * DAY;
  });

  let diff = $derived.by(() => {
    const from = baselineTs;
    const added = [];
    const modified = [];
    const removed = [];
    for (const n of nodes) {
      if (n.deleted_at != null) {
        if (n.deleted_at > from) removed.push(n);
        continue;
      }
      if ((n.first_seen_at ?? 0) > from) {
        added.push(n);
      } else if ((n.last_modified_at ?? 0) > from) {
        modified.push(n);
      }
    }
    const byName = (a, b) => (a.name ?? '').localeCompare(b.name ?? '');
    added.sort(byName);
    modified.sort(byName);
    removed.sort(byName);
    return { added, modified, removed };
  });

  let baselineOptions = $derived(
    deltas.slice(1).map((d, i) => ({
      index: i,
      label: `${shortSha(d.commit_sha)} — ${new Date(d.timestamp * 1000).toLocaleDateString()}`,
      timestamp: d.timestamp,
    }))
  );

  function shortSha(sha) {
    return typeof sha === 'string' && sha.length >= 7 ? sha.slice(0, 7) : (sha ?? '???');
  }

  function emit(domEvent, node) {
    dispatchViewEvent(domEvent.currentTarget, viewEventFromDom(domEvent, 'node', node.id, node));
  }

  function cardClick(domEvent, node) {
    emit(domEvent, node);
    onSelectNode?.(node);
  }

  function cardKeydown(e, node) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      cardClick(e, node);
    }
  }
</script>

<div class="diff-view">
  <div class="diff-header">
    <h3 class="diff-title">Structural Diff</h3>
    {#if repoId && loading}
      <span class="diff-hint" role="status">Loading timeline…</span>
    {:else if error}
      <span class="diff-hint diff-error" role="alert">Timeline unavailable: {error}</span>
    {:else if baselineOptions.length > 0}
      <label class="diff-baseline">
        Baseline
        <select
          value={baselineIndex}
          onchange={(e) => (baselineIndex = Number(e.target.value))}
          aria-label="Diff baseline point"
        >
          {#each baselineOptions as opt (opt.index)}
            <option value={opt.index}>{opt.label}</option>
          {/each}
        </select>
      </label>
    {:else}
      <span class="diff-hint">No timeline deltas — comparing the last 7 days.</span>
    {/if}
    <span class="diff-summary" aria-live="polite">
      <span class="diff-hint">changes through {compareLabel}</span>
      <span class="diff-count add">+{diff.added.length} added</span>
      <span class="diff-count mod">~{diff.modified.length} modified</span>
      <span class="diff-count del">−{diff.removed.length} removed</span>
    </span>
  </div>

  {#if diff.added.length === 0 && diff.modified.length === 0 && diff.removed.length === 0}
    <div class="diff-empty">
      <p>No structural changes between the selected baseline and the latest state.</p>
    </div>
  {:else}
    <div class="diff-columns">
      <section class="diff-col" aria-label="Added nodes">
        <h4 class="diff-col-title add">Added ({diff.added.length})</h4>
        {#each diff.added as node (node.id)}
          <div
            class="diff-card"
            role="button"
            tabindex="0"
            onclick={(e) => cardClick(e, node)}
            ondblclick={(e) => emit(e, node)}
            onmouseenter={(e) => emit(e, node)}
            oncontextmenu={(e) => { e.preventDefault(); emit(e, node); }}
            onkeydown={(e) => cardKeydown(e, node)}
          >
            <Badge value={node.node_type ?? '?'} />
            <span class="diff-card-name" title={node.qualified_name ?? node.name}>{node.name}</span>
            <span class="diff-card-file">{node.file_path ?? ''}</span>
          </div>
        {/each}
      </section>
      <section class="diff-col" aria-label="Modified nodes">
        <h4 class="diff-col-title mod">Modified ({diff.modified.length})</h4>
        {#each diff.modified as node (node.id)}
          <div
            class="diff-card"
            role="button"
            tabindex="0"
            onclick={(e) => cardClick(e, node)}
            ondblclick={(e) => emit(e, node)}
            onmouseenter={(e) => emit(e, node)}
            oncontextmenu={(e) => { e.preventDefault(); emit(e, node); }}
            onkeydown={(e) => cardKeydown(e, node)}
          >
            <Badge value={node.node_type ?? '?'} />
            <span class="diff-card-name" title={node.qualified_name ?? node.name}>{node.name}</span>
            <span class="diff-card-file">{node.file_path ?? ''}</span>
          </div>
        {/each}
      </section>
      <section class="diff-col" aria-label="Removed nodes">
        <h4 class="diff-col-title del">Removed ({diff.removed.length})</h4>
        {#each diff.removed as node (node.id)}
          <div
            class="diff-card removed"
            role="button"
            tabindex="0"
            onclick={(e) => cardClick(e, node)}
            ondblclick={(e) => emit(e, node)}
            onmouseenter={(e) => emit(e, node)}
            oncontextmenu={(e) => { e.preventDefault(); emit(e, node); }}
            onkeydown={(e) => cardKeydown(e, node)}
          >
            <Badge value={node.node_type ?? '?'} />
            <span class="diff-card-name" title={node.qualified_name ?? node.name}>{node.name}</span>
            <span class="diff-card-file">{node.file_path ?? ''}</span>
          </div>
        {/each}
      </section>
    </div>
  {/if}
</div>

<style>
  .diff-view {
    display: flex;
    flex-direction: column;
    gap: 12px;
    height: 100%;
    min-height: 240px;
    overflow-y: auto;
  }
  .diff-header {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
  }
  .diff-title {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    color: var(--color-text, #e2e8f0);
  }
  .diff-hint {
    font-size: 12.5px;
    color: var(--color-text-muted, #94a3b8);
  }
  .diff-error {
    color: #f87171;
  }
  .diff-baseline {
    font-size: 12.5px;
    color: var(--color-text-muted, #94a3b8);
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .diff-baseline select {
    background: var(--color-bg, #0f172a);
    color: var(--color-text, #e2e8f0);
    border: 1px solid #334155;
    border-radius: 6px;
    padding: 2px 6px;
  }
  .diff-summary {
    margin-left: auto;
    display: inline-flex;
    gap: 10px;
    font-size: 12.5px;
  }
  .diff-count.add { color: #22c55e; }
  .diff-count.mod { color: #fbbf24; }
  .diff-count.del { color: #f87171; }

  .diff-empty {
    padding: 32px;
    text-align: center;
    color: var(--color-text-muted, #94a3b8);
    border: 1px dashed #334155;
    border-radius: 8px;
  }

  .diff-columns {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 12px;
    align-items: start;
  }
  @media (max-width: 900px) {
    .diff-columns {
      grid-template-columns: 1fr;
    }
  }
  .diff-col {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }
  .diff-col-title {
    margin: 0 0 2px;
    font-size: 12.5px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .diff-col-title.add { color: #22c55e; }
  .diff-col-title.mod { color: #fbbf24; }
  .diff-col-title.del { color: #f87171; }

  .diff-card {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 10px;
    border: 1px solid #334155;
    border-left: 3px solid #64748b;
    border-radius: 7px;
    background: var(--color-bg, #0f172a);
    cursor: pointer;
    transition: border-color 120ms ease, background-color 120ms ease;
  }
  .diff-card:hover,
  .diff-card:focus-visible {
    background: #1e293b;
    outline: none;
  }
  /* Section accent via column-scoped attribute selectors is not possible in
     scoped Svelte CSS for sibling content, so accent per card kind: */
  .diff-col[aria-label='Added nodes'] .diff-card { border-left-color: #22c55e; }
  .diff-col[aria-label='Modified nodes'] .diff-card { border-left-color: #fbbf24; }
  .diff-col[aria-label='Removed nodes'] .diff-card { border-left-color: #f87171; }
  .diff-card.removed {
    opacity: 0.75;
    text-decoration: line-through;
  }
  .diff-card-name {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 13px;
    color: var(--color-text, #e2e8f0);
  }
  .diff-card-file {
    flex: 0 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
    color: var(--color-text-muted, #94a3b8);
    font-family: 'SF Mono', Menlo, monospace;
  }
</style>
