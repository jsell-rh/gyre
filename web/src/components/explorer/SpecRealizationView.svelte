<script>
  /**
   * Spec Realization View (HSI §3 Default Views) — "what implements this spec?"
   *
   * Two-panel layout: spec list on the left (status badge + task progress),
   * linked graph nodes on the right. Nodes are linked to the selected spec
   * via GovernedBy edges (canonical) or the node's spec_path/spec_paths
   * fields. All data is derived automatically from the knowledge graph —
   * no LLM involved.
   */
  import { api } from '../../lib/api.js';
  import Skeleton from '../../lib/Skeleton.svelte';
  import EmptyState from '../../lib/EmptyState.svelte';

  let {
    repoId = '',
    graph = null,
    onOpenSpec = null,     // (specPath) => void — read/edit the spec
    onSelectNode = null,   // (node) => void — view implementation details
  } = $props();

  let specs = $state([]);
  let specsLoading = $state(false);
  let specsError = $state(null);
  let selectedSpecPath = $state(null);
  let specProgress = $state(null);      // { completed_tasks, total_tasks, ... } for selected spec
  let specProgressLoading = $state(false);
  let filterText = $state('');

  const STATUS_META = {
    approved: { icon: '✓', cls: 'st-approved', label: 'Approved' },
    pending: { icon: '…', cls: 'st-pending', label: 'Pending' },
    draft: { icon: '✎', cls: 'st-draft', label: 'Draft' },
    rejected: { icon: '✕', cls: 'st-rejected', label: 'Rejected' },
    deprecated: { icon: '⌫', cls: 'st-deprecated', label: 'Deprecated' },
    unknown: { icon: '?', cls: 'st-unknown', label: 'Unknown' },
  };

  function statusMeta(status) {
    return STATUS_META[status] ?? STATUS_META.unknown;
  }

  /** Normalize a spec path for comparison: ledger paths and graph spec_path
   *  fields may differ by a leading "specs/" segment. */
  function normPath(p) {
    if (!p) return '';
    let s = String(p).replace(/\\/g, '/').replace(/^\.?\//, '');
    if (s.startsWith('specs/')) s = s.slice('specs/'.length);
    return s.replace(/\/+$/, '');
  }

  // ── GovernedBy index: node id → set of normalized spec paths ───────────
  let governedByIndex = $derived.by(() => {
    const idx = new Map(); // node id → Set<normalized spec path>
    const nodes = graph?.nodes ?? [];
    const edges = graph?.edges ?? [];
    const nodeById = new Map(nodes.map((n) => [n.id, n]));
    for (const e of edges) {
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et !== 'governed_by' || e.deleted_at) continue;
      const src = e.source_id ?? e.source;
      const tgt = e.target_id ?? e.target;
      if (!src) continue;
      // The edge target may be a spec node id (resolve to its path) or carry
      // the spec path directly in metadata/target string.
      let specPath = null;
      const tgtNode = nodeById.get(tgt);
      if (tgtNode && (tgtNode.node_type ?? '').toLowerCase() === 'spec') {
        specPath = tgtNode.spec_path ?? tgtNode.file_path ?? tgtNode.qualified_name ?? tgtNode.name;
      } else if (typeof tgt === 'string' && tgt.includes('/')) {
        // Target id is itself a spec path string.
        specPath = tgt;
      } else if (e.metadata) {
        try {
          const meta = typeof e.metadata === 'string' ? JSON.parse(e.metadata) : e.metadata;
          specPath = meta?.spec_path ?? meta?.spec ?? null;
        } catch {
          specPath = null;
        }
      }
      if (!specPath) continue;
      if (!idx.has(src)) idx.set(src, new Set());
      idx.get(src).add(normPath(specPath));
    }
    return idx;
  });

  /** Is a graph node linked to the given (normalized) spec path? */
  function nodeLinkedTo(node, normSpecPath) {
    if (!node || !normSpecPath) return false;
    if (normPath(node.spec_path) === normSpecPath) return true;
    if (Array.isArray(node.spec_paths) && node.spec_paths.some((p) => normPath(p) === normSpecPath)) {
      return true;
    }
    const viaEdge = governedByIndex.get(node.id);
    if (viaEdge && viaEdge.has(normSpecPath)) return true;
    return false;
  }

  // ── Spec → linked nodes (for the right panel) ──────────────────────────
  let linkedNodes = $derived.by(() => {
    if (!selectedSpecPath) return [];
    const norm = normPath(selectedSpecPath);
    // Spec nodes themselves are not realization entries — the panel answers
    // "what implements this spec?", so only code nodes are listed.
    return (graph?.nodes ?? []).filter(
      (n) => String(n.node_type ?? '').toLowerCase() !== 'spec' && nodeLinkedTo(n, norm)
    );
  });

  // ── Linked-node counts per spec (for the left panel badges) ────────────
  let linkedCounts = $derived.by(() => {
    const counts = new Map(); // spec path → node count
    const nodes = (graph?.nodes ?? []).filter(
      (n) => String(n.node_type ?? '').toLowerCase() !== 'spec'
    );
    for (const node of nodes) {
      const paths = new Set();
      if (node.spec_path) paths.add(normPath(node.spec_path));
      if (Array.isArray(node.spec_paths)) node.spec_paths.forEach((p) => paths.add(normPath(p)));
      const viaEdge = governedByIndex.get(node.id);
      if (viaEdge) viaEdge.forEach((p) => paths.add(p));
      for (const p of paths) counts.set(p, (counts.get(p) ?? 0) + 1);
    }
    return counts;
  });

  let filteredSpecs = $derived.by(() => {
    const q = filterText.trim().toLowerCase();
    if (!q) return specs;
    return specs.filter(
      (s) =>
        (s.path ?? '').toLowerCase().includes(q) ||
        (s.title ?? '').toLowerCase().includes(q)
    );
  });

  $effect(() => {
    const id = repoId;
    if (!id) {
      specs = [];
      return;
    }
    specsLoading = true;
    specsError = null;
    api
      .getSpecs()
      .then((d) => {
        specs = Array.isArray(d) ? d : [];
      })
      .catch((e) => {
        specsError = e?.message ?? String(e);
        specs = [];
      })
      .finally(() => { specsLoading = false; });
  });

  // Auto-select the first spec (prefer one with linked nodes in this repo).
  $effect(() => {
    if (selectedSpecPath || specsLoading || specs.length === 0) return;
    const withLinks = specs.find(
      (s) => (linkedCounts.get(normPath(s.path)) ?? 0) > 0
    );
    selectedSpecPath = (withLinks ?? specs[0]).path;
  });

  // Load task progress for the selected spec.
  $effect(() => {
    const path = selectedSpecPath;
    if (!path) {
      specProgress = null;
      return;
    }
    specProgressLoading = true;
    api
      .getSpecProgress(path)
      .then((p) => { specProgress = p; })
      .catch(() => { specProgress = null; })
      .finally(() => { specProgressLoading = false; });
  });

  function shortPath(p) {
    const parts = String(p ?? '').split('/');
    return parts[parts.length - 1] || p;
  }

  function progressPct(p) {
    const total = p?.total_tasks ?? (Array.isArray(p?.tasks) ? p.tasks.length : 0);
    const done = p?.completed_tasks ??
      (Array.isArray(p?.tasks)
        ? p.tasks.filter((t) => t.status === 'done' || t.status === 'completed').length
        : 0);
    if (!total) return null;
    return Math.round((done / total) * 100);
  }

  function nodeTypeLabel(t) {
    return String(t ?? 'node').toLowerCase();
  }
</script>

<div class="spec-realization" data-testid="spec-realization-view">
  <!-- Left panel: spec list -->
  <section class="sr-left" aria-label="Specs">
    <header class="sr-panel-header">
      <h2 class="sr-title">Specs</h2>
      <input
        type="search"
        class="sr-filter"
        placeholder="Filter specs…"
        bind:value={filterText}
        aria-label="Filter specs"
      />
    </header>
    {#if specsLoading}
      <div class="sr-list-loading">
        {#each Array(5) as _}<Skeleton width="100%" height="3rem" />{/each}
      </div>
    {:else if specsError}
      <div class="sr-error" role="alert">
        <p>Failed to load specs: {specsError}</p>
      </div>
    {:else if filteredSpecs.length === 0}
      <EmptyState title="No specs found" description={filterText ? 'No specs match the filter.' : 'No specs are registered yet.'} />
    {:else}
      <ul class="sr-list">
        {#each filteredSpecs as spec (spec.path)}
          {@const meta = statusMeta(spec.approval_status)}
          {@const count = linkedCounts.get(normPath(spec.path)) ?? 0}
          <li>
            <button
              type="button"
              class="sr-spec-item"
              class:selected={spec.path === selectedSpecPath}
              onclick={() => { selectedSpecPath = spec.path; }}
              aria-pressed={spec.path === selectedSpecPath}
            >
              <span class="sr-spec-name" title={spec.path}>{shortPath(spec.path)}</span>
              <span class="sr-spec-badges">
                <span class="sr-status {meta.cls}" title={meta.label}>{meta.icon} {meta.label}</span>
                <span class="sr-linked-count" title="Linked graph nodes">{count} node{count !== 1 ? 's' : ''}</span>
              </span>
              {#if spec.repo_id}<span class="sr-spec-repo mono">{spec.repo_id}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <!-- Right panel: realization (linked nodes) -->
  <section class="sr-right" aria-label="Realization">
    {#if !selectedSpecPath}
      <EmptyState title="Select a spec" description="Pick a spec on the left to see what implements it." />
    {:else}
      {@const selectedSpec = specs.find((s) => s.path === selectedSpecPath)}
      <header class="sr-panel-header">
        <h2 class="sr-title">Realization</h2>
        <div class="sr-actions">
          <button
            type="button"
            class="sr-action-btn"
            onclick={() => onOpenSpec?.(selectedSpecPath)}
            title={selectedSpecPath}
          >Read / Edit Spec</button>
        </div>
      </header>
      {#if selectedSpec}
        <div class="sr-selected-header">
          <span class="sr-selected-path mono" title={selectedSpec.path}>{selectedSpec.path}</span>
          {#if selectedSpec.title}<span class="sr-selected-title">{selectedSpec.title}</span>{/if}
          {#if specProgressLoading}
            <span class="sr-progress-loading">loading task progress…</span>
          {:else if specProgress}
            {@const pct = progressPct(specProgress)}
            {#if pct != null}
              <span class="sr-progress">
                <span class="sr-progress-bar"><span class="sr-progress-fill" style="width:{pct}%"></span></span>
                <span class="sr-progress-label">
                  Implementation: {specProgress.completed_tasks ?? 0}/{specProgress.total_tasks ?? (Array.isArray(specProgress.tasks) ? specProgress.tasks.length : 0)} tasks
                </span>
              </span>
            {/if}
          {/if}
        </div>
      {/if}
      {#if linkedNodes.length === 0}
        <EmptyState
          title="No linked nodes"
          description="No graph nodes in this repository are linked to the selected spec (no GovernedBy edges or spec_path matches)."
        />
      {:else}
        <ul class="sr-nodes" data-count={linkedNodes.length}>
          {#each linkedNodes as node (node.id)}
            <li>
              <button
                type="button"
                class="sr-node"
                onclick={() => onSelectNode?.(node)}
                title={node.qualified_name ?? node.name}
              >
                <span class="sr-node-type">{nodeTypeLabel(node.node_type)}</span>
                <span class="sr-node-name">{node.name}</span>
                <span class="sr-node-qn mono">{node.qualified_name ?? ''}</span>
                {#if node.spec_confidence && node.spec_confidence !== 'none'}
                  <span class="sr-node-confidence">confidence: {node.spec_confidence}</span>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  </section>
</div>

<style>
  .spec-realization {
    display: grid;
    grid-template-columns: minmax(240px, 340px) 1fr;
    gap: 1rem;
    height: 100%;
    min-height: 0;
    padding: 1rem 1.25rem;
    box-sizing: border-box;
  }
  .sr-left, .sr-right {
    display: flex; flex-direction: column; gap: 0.5rem; min-height: 0;
    border: 1px solid var(--border, #30363d); border-radius: 10px;
    background: var(--bg-elevated, #161b22); padding: 0.75rem; overflow-y: auto;
  }
  .sr-panel-header { display: flex; align-items: center; justify-content: space-between; gap: 0.5rem; flex-wrap: wrap; }
  .sr-title { margin: 0; font-size: 0.95rem; font-weight: 600; }
  .sr-filter {
    background: var(--bg, #0d1117); border: 1px solid var(--border, #30363d);
    border-radius: 6px; color: inherit; padding: 0.3rem 0.6rem; font-size: 0.8rem;
  }
  .sr-list-loading { display: flex; flex-direction: column; gap: 0.5rem; }
  .sr-error { color: #f85149; font-size: 0.85rem; }
  .sr-list, .sr-nodes { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 0.4rem; }
  .sr-spec-item {
    display: flex; flex-direction: column; gap: 0.3rem; width: 100%;
    padding: 0.55rem 0.7rem; border-radius: 8px; border: 1px solid transparent;
    background: none; color: inherit; text-align: left; cursor: pointer; font: inherit;
  }
  .sr-spec-item:hover { background: rgba(88, 166, 255, 0.05); }
  .sr-spec-item.selected { border-color: rgba(88, 166, 255, 0.5); background: rgba(88, 166, 255, 0.08); }
  .sr-spec-name { font-weight: 600; font-size: 0.85rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sr-spec-badges { display: flex; gap: 0.5rem; flex-wrap: wrap; font-size: 0.7rem; }
  .sr-status { padding: 0.05rem 0.45rem; border-radius: 10px; }
  .st-approved { background: rgba(63, 185, 80, 0.15); color: #3fb950; }
  .st-pending { background: rgba(210, 153, 34, 0.15); color: #d29922; }
  .st-draft { background: rgba(139, 148, 158, 0.15); color: #8b949e; }
  .st-rejected { background: rgba(248, 81, 73, 0.15); color: #f85149; }
  .st-deprecated { background: rgba(139, 148, 158, 0.15); color: #8b949e; text-decoration: line-through; }
  .st-unknown { background: rgba(139, 148, 158, 0.1); color: #8b949e; }
  .sr-linked-count { color: var(--text-secondary, #8b949e); }
  .sr-spec-repo { font-size: 0.65rem; color: var(--text-secondary, #8b949e); font-family: ui-monospace, monospace; }
  .sr-actions { display: flex; gap: 0.4rem; }
  .sr-action-btn {
    background: rgba(88, 166, 255, 0.1); border: 1px solid rgba(88, 166, 255, 0.4);
    color: #58a6ff; border-radius: 6px; padding: 0.25rem 0.7rem; font-size: 0.75rem;
    cursor: pointer;
  }
  .sr-action-btn:hover { background: rgba(88, 166, 255, 0.2); }
  .sr-selected-header { display: flex; flex-direction: column; gap: 0.2rem; padding-bottom: 0.5rem; border-bottom: 1px solid var(--border, #30363d); }
  .sr-selected-path { font-size: 0.8rem; color: var(--text-secondary, #8b949e); font-family: ui-monospace, monospace; word-break: break-all; }
  .sr-selected-title { font-size: 0.9rem; font-weight: 600; }
  .sr-progress { display: flex; align-items: center; gap: 0.5rem; margin-top: 0.25rem; }
  .sr-progress-bar { width: 120px; height: 6px; border-radius: 3px; background: var(--border, #30363d); overflow: hidden; display: inline-block; }
  .sr-progress-fill { display: block; height: 100%; background: #3fb950; }
  .sr-progress-label, .sr-progress-loading { font-size: 0.72rem; color: var(--text-secondary, #8b949e); }
  .sr-node {
    display: grid; grid-template-columns: auto 1fr; gap: 0.15rem 0.6rem; width: 100%;
    padding: 0.5rem 0.7rem; border: 1px solid var(--border, #30363d); border-radius: 8px;
    background: none; color: inherit; text-align: left; cursor: pointer; font: inherit;
  }
  .sr-node:hover { border-color: rgba(88, 166, 255, 0.5); background: rgba(88, 166, 255, 0.05); }
  .sr-node-type {
    grid-row: span 3; align-self: start; padding: 0.1rem 0.5rem; border-radius: 10px;
    background: rgba(88, 166, 255, 0.12); color: #58a6ff; font-size: 0.7rem;
    text-transform: capitalize; height: fit-content;
  }
  .sr-node-name { font-weight: 600; font-size: 0.85rem; }
  .sr-node-qn { font-size: 0.7rem; color: var(--text-secondary, #8b949e); font-family: ui-monospace, monospace; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sr-node-confidence { font-size: 0.68rem; color: var(--text-secondary, #8b949e); }
  .mono { font-family: ui-monospace, monospace; }
  @media (max-width: 800px) {
    .spec-realization { grid-template-columns: 1fr; grid-template-rows: auto 1fr; }
  }
</style>
