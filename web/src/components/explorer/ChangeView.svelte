<script>
  /**
   * Change View (HSI §3 Default Views) — "what changed since my last visit?"
   *
   * Powered by architectural deltas: queries
   *   GET /api/v1/repos/:id/graph/timeline?since=<last_seen_at>
   * where `since` comes from the client-cached `X-Gyre-Last-Seen` header
   * (web/src/lib/last-seen.js — snapshot from the previous visit, 24h fallback).
   *
   * Each delta's `delta_json` carries:
   *   nodes_added:    [{name, node_type, qualified_name}]
   *   nodes_modified: [{qualified_name, field_changes: [{field, old_value, new_value}]}]
   *   nodes_removed:  [qualified_name, ...]
   * Rendered as Added (+) / Modified (~) / Removed (-) entries showing node
   * name, type, containing module, spec reference and authoring agent.
   * Entries are clickable and navigate to the node in the Boundary View.
   */
  import { api } from '../../lib/api.js';
  import { getChangeViewSince, getLastSeenAtVisitStart } from '../../lib/last-seen.js';
  import Skeleton from '../../lib/Skeleton.svelte';
  import EmptyState from '../../lib/EmptyState.svelte';

  let { repoId = '', graph = null, onNavigateToNode = null } = $props();

  let since = $state(null);
  let deltas = $state([]);
  let entries = $state([]);
  let loading = $state(false);
  let error = $state(null);
  let expandedEntry = $state(null); // entry id whose field-changes are expanded

  const KIND_META = {
    added: { symbol: '+', label: 'Added', cls: 'change-added' },
    modified: { symbol: '~', label: 'Modified', cls: 'change-modified' },
    removed: { symbol: '-', label: 'Removed', cls: 'change-removed' },
  };

  function formatSince(ts) {
    if (!ts) return '';
    const d = new Date(ts * 1000);
    return d.toLocaleString(undefined, {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
    });
  }

  function formatTime(ts) {
    if (!ts) return '';
    return new Date(ts * 1000).toLocaleString(undefined, {
      month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
    });
  }

  /** Containing module for a node: via Contains edges in the graph, else the
   *  qualified-name prefix (crate::module::item → crate::module). */
  function moduleFor(qualifiedName, nodeId) {
    const nodes = graph?.nodes ?? [];
    const edges = graph?.edges ?? [];
    if (nodeId) {
      const contains = edges.find(
        (e) =>
          !e.deleted_at &&
          (e.edge_type ?? e.type ?? '').toLowerCase() === 'contains' &&
          (e.target_id ?? e.target) === nodeId
      );
      if (contains) {
        const parent = nodes.find((n) => n.id === (contains.source_id ?? contains.source));
        if (parent) return parent.qualified_name ?? parent.name;
      }
    }
    const parts = String(qualifiedName ?? '').split(/[.:/]/);
    return parts.length > 1 ? parts.slice(0, -1).join('::') : '';
  }

  function nodeName(qualifiedName) {
    const parts = String(qualifiedName ?? '').split(/[.:/]/);
    return parts[parts.length - 1] || qualifiedName;
  }

  /** Parse one architectural delta into flat display entries. */
  function parseDelta(delta) {
    let parsed = {};
    try {
      parsed = JSON.parse(delta.delta_json ?? '{}');
    } catch {
      parsed = {};
    }
    const out = [];
    const base = {
      timestamp: delta.timestamp ?? 0,
      specRef: delta.spec_ref ?? null,
      agentId: delta.agent_id ?? null,
      commitSha: delta.commit_sha ?? null,
    };
    const added = Array.isArray(parsed.nodes_added) ? parsed.nodes_added : [];
    for (const n of added) {
      out.push({
        ...base,
        id: `${delta.id}:a:${n.qualified_name}`,
        kind: 'added',
        name: n.name ?? nodeName(n.qualified_name),
        nodeType: n.node_type ?? null,
        qualifiedName: n.qualified_name ?? null,
        fieldChanges: null,
      });
    }
    const modified = Array.isArray(parsed.nodes_modified) ? parsed.nodes_modified : [];
    for (const m of modified) {
      out.push({
        ...base,
        id: `${delta.id}:m:${m.qualified_name}`,
        kind: 'modified',
        name: nodeName(m.qualified_name),
        nodeType: null,
        qualifiedName: m.qualified_name ?? null,
        fieldChanges: Array.isArray(m.field_changes) ? m.field_changes : [],
      });
    }
    const removed = Array.isArray(parsed.nodes_removed) ? parsed.nodes_removed : [];
    for (const qn of removed) {
      out.push({
        ...base,
        id: `${delta.id}:r:${qn}`,
        kind: 'removed',
        name: nodeName(qn),
        nodeType: null,
        qualifiedName: qn,
        fieldChanges: null,
      });
    }
    return out;
  }

  /** Fetch the delta timeline for the current repo (used on mount and retry). */
  function fetchTimeline(repoIdArg, sinceValue) {
    loading = true;
    error = null;
    api
      .repoGraphTimeline(repoIdArg, sinceValue)
      .then((d) => {
        deltas = Array.isArray(d) ? d : [];
        entries = deltas.flatMap(parseDelta)
          .sort((a, b) => (b.timestamp ?? 0) - (a.timestamp ?? 0));
      })
      .catch((e) => {
        error = e?.message ?? String(e);
        deltas = [];
        entries = [];
      })
      .finally(() => { loading = false; });
  }

  $effect(() => {
    const id = repoId;
    if (!id) {
      deltas = [];
      entries = [];
      return;
    }
    // Snapshot `since` once per repo selection — later responses (this view's
    // own fetch included) update the cache, but "since last visit" must stay
    // anchored to the visit start. NOTE: use a local for the query — reading
    // the `since` state after assigning it inside this effect would create a
    // self-dependency and re-trigger the fetch.
    const sinceValue = getChangeViewSince();
    since = sinceValue;
    fetchTimeline(id, sinceValue);
  });

  /** Resolve a delta entry back to a live graph node (Boundary navigation). */
  function graphNodeFor(entry) {
    if (!entry?.qualifiedName) return null;
    return (graph?.nodes ?? []).find((n) => n.qualified_name === entry.qualifiedName) ?? null;
  }

  function handleEntryClick(entry) {
    if (entry.fieldChanges?.length) {
      expandedEntry = expandedEntry === entry.id ? null : entry.id;
      return;
    }
    const node = graphNodeFor(entry);
    if (node && onNavigateToNode) onNavigateToNode(node);
  }
</script>

<div class="change-view" data-testid="change-view" aria-busy={loading}>
  <div class="change-header">
    <h2 class="change-title">Since your last visit</h2>
    <p class="change-since" data-testid="change-since">
      {#if getLastSeenAtVisitStart() != null}
        Showing structural changes since {formatSince(since)}
      {:else}
        First visit on this browser — showing changes from the last 24 hours
      {/if}
    </p>
  </div>

  {#if !repoId}
    <EmptyState title="No repository selected" description="Select a repository to see what changed." />
  {:else if loading}
    <div class="change-loading" aria-live="polite">
      {#each Array(6) as _}
        <Skeleton width="100%" height="3rem" />
      {/each}
    </div>
  {:else if error}
    <div class="change-error" role="alert">
      <p>Failed to load changes: {error}</p>
      <button type="button" onclick={() => fetchTimeline(repoId, since)}>
        Retry
      </button>
    </div>
  {:else if entries.length === 0}
    <EmptyState
      title="No structural changes"
      description="No architectural deltas were recorded since your last visit."
    />
  {:else}
    <ul class="change-list" data-count={entries.length}>
      {#each entries as entry (entry.id)}
        {@const meta = KIND_META[entry.kind]}
        {@const node = entry.kind === 'removed' ? null : graphNodeFor(entry)}
        <li class="change-entry {meta.cls}" data-kind={entry.kind}>
          <button
            type="button"
            class="change-entry-btn"
            class:resolvable={!!node}
            onclick={() => handleEntryClick(entry)}
            aria-label={`${meta.label} ${entry.name}`}
            title={node ? 'View in Boundary View' : (entry.fieldChanges?.length ? 'Toggle field changes' : entry.qualifiedName)}
          >
            <span class="change-symbol" aria-hidden="true">{meta.symbol}</span>
            <span class="change-entry-main">
              <span class="change-entry-name">{entry.name}</span>
              <span class="change-entry-meta">
                {#if entry.nodeType}<span class="change-type">{entry.nodeType}</span>{/if}
                {#if entry.qualifiedName && moduleFor(entry.qualifiedName, node?.id)}
                  <span class="change-module">in {moduleFor(entry.qualifiedName, node?.id)}</span>
                {/if}
                {#if entry.kind === 'modified' && entry.fieldChanges?.length}
                  <span class="change-fields">({entry.fieldChanges.length} field{entry.fieldChanges.length !== 1 ? 's' : ''} changed)</span>
                {/if}
              </span>
              <span class="change-entry-sub">
                {#if entry.specRef}<span class="change-spec">Spec: {entry.specRef}</span>{/if}
                {#if entry.agentId}<span class="change-agent">Agent: {entry.agentId}</span>{/if}
                <span class="change-time">{formatTime(entry.timestamp)}</span>
              </span>
            </span>
          </button>
          {#if entry.kind === 'modified' && expandedEntry === entry.id && entry.fieldChanges?.length}
            <ul class="change-field-list">
              {#each entry.fieldChanges as fc, i (i)}
                <li class="change-field">
                  <span class="change-field-name mono">{fc.field}</span>
                  {#if fc.old_value != null}<span class="change-field-old mono">{fc.old_value}</span>→{/if}
                  <span class="change-field-new mono">{fc.new_value}</span>
                </li>
              {/each}
            </ul>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .change-view {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    height: 100%;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    box-sizing: border-box;
  }
  .change-header { display: flex; flex-direction: column; gap: 0.25rem; }
  .change-title { margin: 0; font-size: 1.1rem; font-weight: 600; }
  .change-since { margin: 0; font-size: 0.85rem; color: var(--text-secondary, #8b949e); }
  .change-loading { display: flex; flex-direction: column; gap: 0.5rem; }
  .change-error {
    display: flex; align-items: center; gap: 1rem; justify-content: space-between;
    background: rgba(248, 81, 73, 0.08); border: 1px solid rgba(248, 81, 73, 0.4);
    border-radius: 8px; padding: 0.75rem 1rem; color: #f85149;
  }
  .change-error p { margin: 0; font-size: 0.85rem; }
  .change-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 0.5rem; }
  .change-entry { border: 1px solid var(--border, #30363d); border-radius: 8px; background: var(--bg-elevated, #161b22); overflow: hidden; }
  .change-entry-btn {
    display: flex; align-items: flex-start; gap: 0.75rem; width: 100%;
    padding: 0.6rem 0.9rem; background: none; border: none; cursor: default;
    text-align: left; color: inherit; font: inherit;
  }
  .change-entry-btn.resolvable { cursor: pointer; }
  .change-entry-btn.resolvable:hover { background: rgba(88, 166, 255, 0.06); }
  .change-symbol {
    flex-shrink: 0; width: 1.4rem; height: 1.4rem; border-radius: 50%;
    display: inline-flex; align-items: center; justify-content: center;
    font-weight: 700; font-size: 0.95rem; line-height: 1;
  }
  .change-added .change-symbol { background: rgba(63, 185, 80, 0.15); color: #3fb950; }
  .change-modified .change-symbol { background: rgba(210, 153, 34, 0.15); color: #d29922; }
  .change-removed .change-symbol { background: rgba(248, 81, 73, 0.15); color: #f85149; }
  .change-added { border-left: 3px solid #3fb950; }
  .change-modified { border-left: 3px solid #d29922; }
  .change-removed { border-left: 3px solid #f85149; }
  .change-entry-main { display: flex; flex-direction: column; gap: 0.15rem; min-width: 0; flex: 1; }
  .change-entry-name { font-weight: 600; font-size: 0.9rem; }
  .change-entry-meta { display: flex; gap: 0.5rem; flex-wrap: wrap; font-size: 0.75rem; color: var(--text-secondary, #8b949e); }
  .change-type { text-transform: capitalize; }
  .change-entry-sub { display: flex; gap: 0.75rem; flex-wrap: wrap; font-size: 0.72rem; color: var(--text-secondary, #8b949e); }
  .change-spec, .change-agent { font-family: ui-monospace, monospace; }
  .change-field-list { list-style: none; margin: 0; padding: 0.4rem 0.9rem 0.6rem 3rem; display: flex; flex-direction: column; gap: 0.25rem; }
  .change-field { font-size: 0.75rem; display: flex; gap: 0.5rem; flex-wrap: wrap; align-items: baseline; }
  .change-field-name { color: var(--text-secondary, #8b949e); }
  .change-field-old { text-decoration: line-through; color: #f85149; opacity: 0.8; }
  .change-field-new { color: #3fb950; }
  .mono { font-family: ui-monospace, monospace; }
</style>
