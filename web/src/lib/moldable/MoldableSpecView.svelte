<script>
  /**
   * MoldableSpecView — system-explorer.md §2 "Spec View (Inline)".
   *
   * Spec content (fetched from the spec ledger via GET /specs/:path,
   * rendered as markdown) alongside the implementation realization: which
   * code elements implement this spec (✓ present in the current extraction,
   * ✗ soft-deleted), completeness percentage, stored assertion results, and
   * the [Edit] [History] actions. Edit opens the inline spec editor
   * (ExplorerView) which carries the progressive Preview / Publish workflow.
   */
  import './shared.css';
  import { api } from '../api.js';
  import { renderMarkdown } from '../markdown.js';
  import {
    computeRelationships,
    computeSpecGovernedNodes,
    computeSpecCompleteness,
    specPathOf,
  } from './model.js';

  let {
    node,
    nodes = [],
    edges = [],
    onNavigate = () => {},
    repoId = null,
    onEditSpec = null,
  } = $props();

  let relationships = $derived(computeRelationships(node, nodes, edges));
  let governedNodes = $derived(computeSpecGovernedNodes(node, nodes, edges));
  let completeness = $derived(computeSpecCompleteness(governedNodes, nodes, edges));

  // Group governed nodes by type for the realization list
  let nodesByType = $derived.by(() => {
    const groups = new Map();
    for (const gn of governedNodes) {
      const t = gn.node_type ?? 'unknown';
      if (!groups.has(t)) groups.set(t, []);
      groups.get(t).push(gn);
    }
    return groups;
  });

  // Spec content from the ledger (async)
  let specDetail = $state(null);
  let contentLoading = $state(false);
  let contentUnavailable = $state(false);
  // Stored assertion results (GET /repos/:id/specs/:path/assertions)
  let assertions = $state(null);
  // Inline history (GET /specs/:path/history)
  let history = $state(null);
  let historyOpen = $state(false);
  let historyLoading = $state(false);

  $effect(() => {
    const path = specPathOf(node);
    const rid = repoId ?? node?.repo_id ?? null;
    specDetail = null;
    contentUnavailable = false;
    assertions = null;
    history = null;
    historyOpen = false;
    if (!path) return;
    contentLoading = true;
    api.specContent(path, rid)
      .then((d) => {
        if (d?.content) specDetail = d;
        else contentUnavailable = true;
      })
      .catch(() => { contentUnavailable = true; })
      .finally(() => { contentLoading = false; });
    if (rid) {
      api.getSpecAssertionResults(rid, path)
        .then((r) => { assertions = Array.isArray(r?.assertions) ? r.assertions : null; })
        .catch(() => { assertions = null; });
    }
  });

  async function toggleHistory() {
    if (historyOpen) { historyOpen = false; return; }
    historyOpen = true;
    if (history) return;
    historyLoading = true;
    try {
      const h = await api.specHistoryRepo(specPathOf(node), repoId ?? node?.repo_id ?? null);
      history = Array.isArray(h) ? h : [];
    } catch {
      history = [];
    } finally {
      historyLoading = false;
    }
  }

  function editSpec() {
    const path = specPathOf(node);
    if (path && onEditSpec) onEditSpec(path);
  }

  function fmtHistoryTime(t) {
    if (!t) return '';
    const d = new Date(typeof t === 'number' ? t * 1000 : t);
    return isNaN(d.getTime()) ? String(t) : d.toLocaleString();
  }

  let assertionPassed = $derived(assertions ? assertions.filter(a => a.passed).length : null);
</script>

<div class="moldable-view moldable-spec-view" data-moldable-view="spec">
  <!-- Spec content (from the ledger, rendered as markdown) -->
  <div class="detail-section">
    <h4 class="detail-section-title">Content</h4>
    {#if contentLoading}
      <p class="spec-content-unavailable">Loading spec content…</p>
    {:else if specDetail?.content}
      <div class="spec-content-full">{@html renderMarkdown(specDetail.content)}</div>
      {#if specDetail.current_sha}
        <p class="detail-modified-summary">Version <code>{specDetail.current_sha.slice(0, 7)}</code></p>
      {/if}
    {:else if node.doc_comment || node.description}
      <div class="spec-content-preview">
        <p class="detail-doc">{node.doc_comment ?? node.description}</p>
      </div>
      <p class="spec-content-unavailable">Full spec content not available — showing extracted preview.</p>
    {:else}
      <p class="spec-content-unavailable">Spec content not available{specPathOf(node) ? ` for ${specPathOf(node)}` : ''}.</p>
    {/if}
  </div>

  <!-- Spec path (when the node identity differs from the ledger path) -->
  {#if specPathOf(node) && specPathOf(node) !== node.file_path}
    <div class="detail-section">
      <h4 class="detail-section-title">Spec Path</h4>
      <p class="detail-file"><code>{specPathOf(node)}</code></p>
    </div>
  {/if}

  <!-- Implementation realization: which code elements implement this spec -->
  {#if completeness}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Implementation Completeness</summary>
      <div class="spec-completeness-meter">
        <div class="completeness-bar">
          <div class="completeness-fill" style="width: {completeness.pct}%"></div>
          <div class="completeness-tested-fill" style="width: {completeness.pct}%"></div>
        </div>
        <div class="completeness-stats">
          <span class="completeness-stat">
            <span class="completeness-stat-value">{completeness.realized}</span>
            <span class="completeness-stat-label">of {completeness.total} realized</span>
          </span>
          <span class="completeness-stat">
            <span class="completeness-stat-value">{completeness.pct}%</span>
            <span class="completeness-stat-label">complete</span>
          </span>
          <span class="completeness-stat">
            <span class="completeness-stat-value">{completeness.tested}</span>
            <span class="completeness-stat-label">tested</span>
          </span>
        </div>
      </div>
      {#if completeness.total === 0}
        <p class="detail-story detail-muted">No implementation nodes linked to this spec.</p>
      {/if}
    </details>
  {/if}

  <!-- Stored assertion results (gyre:assert checks from the last push) -->
  {#if assertions !== null && assertions.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Assertions ({assertionPassed}/{assertions.length} passing)</summary>
      <div class="spec-assertion-summary">
        {#each assertions.slice(0, 8) as a (a.line)}
          <span class="assertion-pill {a.passed ? 'passed' : 'failed'}" title={a.details ?? ''}>
            {#if a.assertion_type}{a.assertion_type}{:else}line {a.line}{/if}
          </span>
        {/each}
        {#if assertions.length > 8}
          <span class="detail-more">+{assertions.length - 8} more</span>
        {/if}
      </div>
    </details>
  {/if}

  <!-- Linked nodes by type (✓ realized / ✗ removed from code) -->
  {#if governedNodes.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Linked Nodes ({governedNodes.length})</summary>
      {#each [...nodesByType.entries()] as [nodeType, typeNodes] (nodeType)}
        <div class="spec-type-group">
          <h5 class="spec-type-group-label">{nodeType} ({typeNodes.length})</h5>
          <ul class="detail-ref-list">
            {#each typeNodes.slice(0, 10) as gn (gn.id)}
              <li>
                <button class="detail-ref-link" onclick={() => onNavigate(gn)} type="button">
                  <span class="ref-type">{gn.node_type}</span>
                  <span class="field-name">{gn.name}</span>
                  {#if gn.deleted_at}
                    <span class="spec-check spec-check-failed" title="Removed from the current extraction">&#10007;</span>
                  {:else}
                    <span class="spec-check" title="Present in the current codebase">&#10003;</span>
                  {/if}
                  {#if gn.test_coverage != null}
                    <span class="spec-coverage-badge" class:coverage-good={gn.test_coverage >= 0.5} class:coverage-poor={gn.test_coverage < 0.5}>
                      {Math.round(gn.test_coverage * 100)}%
                    </span>
                  {/if}
                </button>
              </li>
            {/each}
            {#if typeNodes.length > 10}
              <li class="detail-more">+{typeNodes.length - 10} more</li>
            {/if}
          </ul>
        </div>
      {/each}
    </details>
  {/if}

  <!-- Contains (child specs or sections) -->
  {#if relationships.contains.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Contains ({relationships.contains.length})</summary>
      <ul class="detail-ref-list">
        {#each relationships.contains.slice(0, 15) as c (c.id)}
          <li>
            <button class="detail-ref-link" onclick={() => onNavigate(c)} type="button">
              <span class="ref-type">{c.node_type}</span> {c.name}
            </button>
          </li>
        {/each}
        {#if relationships.contains.length > 15}
          <li class="detail-more">+{relationships.contains.length - 15} more</li>
        {/if}
      </ul>
    </details>
  {/if}

  <!-- Inline history -->
  {#if historyOpen}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">History</summary>
      {#if historyLoading}
        <p class="spec-content-unavailable">Loading history…</p>
      {:else if history.length === 0}
        <p class="detail-story detail-muted">No recorded history for this spec.</p>
      {:else}
        <ul class="spec-history-list">
          {#each history.slice().reverse() as ev, i (i)}
            <li class="spec-history-item">
              <span class="spec-history-event">{ev.event ?? ev.event_type ?? ev.change_type ?? 'change'}</span>
              {#if ev.reason ?? ev.revocation_reason}
                <span class="detail-muted">{ev.reason ?? ev.revocation_reason}</span>
              {/if}
              {#if ev.sha ?? ev.commit_sha}
                <code class="spec-history-sha">{(ev.sha ?? ev.commit_sha).slice(0, 7)}</code>
              {/if}
              <span class="spec-history-time">{fmtHistoryTime(ev.timestamp ?? ev.created_at ?? ev.approved_at)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </details>
  {/if}

  <!-- Actions: Edit opens the inline spec editor (Preview / Publish live
       there); History toggles the inline version list above. -->
  {#if specPathOf(node)}
    <div class="moldable-actions">
      {#if onEditSpec}
        <button class="moldable-action-btn" onclick={editSpec} type="button" title="Open the inline spec editor (edit, preview, publish)">
          Edit
        </button>
      {/if}
      <button class="moldable-action-btn" onclick={toggleHistory} type="button" aria-expanded={historyOpen} title="Show spec version history">
        {historyOpen ? 'Hide History' : 'History'}
      </button>
    </div>
  {/if}
</div>
