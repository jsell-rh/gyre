<script>
  /**
   * MoldableTypeView — system-explorer.md §2 "Type View".
   *
   * Tailored detail view for struct/enum/table/component graph nodes:
   * spec linkage (clickable → opens spec inline), last modified agent + time
   * (agent reference clickable → provenance), fields with clickable type
   * links, Implements / Used by sections, Story (creation history), and the
   * risk footer (churn / coupling / spec coverage / tests).
   */
  import './shared.css';
  import {
    computeRelationships,
    computeRisk,
    computeStory,
    resolveFieldType,
    timeAgo,
  } from './model.js';

  let {
    node,
    nodes = [],
    edges = [],
    onNavigate = () => {},
    onAgentProvenance = null,
  } = $props();

  let relationships = $derived(computeRelationships(node, nodes, edges));
  let risk = $derived(computeRisk(node, relationships));
  let story = $derived(computeStory(node, relationships, edges));

  // Used by: callers + incoming FieldOf edges (types with a field of this type)
  let usedBy = $derived.by(() => {
    if (!node) return [];
    const usedBySet = new Map();
    for (const c of relationships.calledBy) usedBySet.set(c.id, c);
    for (const e of edges) {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et === 'field_of' && tgt === node.id) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) usedBySet.set(srcNode.id, srcNode);
      }
    }
    return [...usedBySet.values()];
  });

  function navigateToSpec() {
    const target = node.spec_path ?? relationships.governedBy;
    onNavigate({ id: target, name: target, node_type: 'spec' });
  }

  function crateOf(n) {
    return n?.qualified_name?.split('::')[0] ?? null;
  }
</script>

<div class="moldable-view moldable-type-view" data-moldable-view="type">
  <!-- Spec linkage (clickable → opens the spec inline) -->
  {#if node.spec_path || relationships.governedBy}
    <div class="detail-section">
      <h4 class="detail-section-title">Spec</h4>
      <button class="detail-spec-button" onclick={navigateToSpec} type="button" title="Open spec inline">
        {node.spec_path ?? relationships.governedBy}
      </button>
    </div>
  {/if}

  <!-- Last modified (agent + time) — the agent reference opens provenance -->
  {#if node.last_modified_by || node.last_modified_at}
    <div class="detail-section">
      <p class="detail-modified-summary">
        {#if node.last_modified_by}
          Last modified by
          {#if onAgentProvenance}
            <button class="agent-ref" onclick={() => onAgentProvenance(node.last_modified_by)} type="button" title="Show agent provenance">{node.last_modified_by}</button>
          {:else}
            <code>{node.last_modified_by}</code>
          {/if}
        {/if}
        {#if node.last_modified_at}{node.last_modified_by ? ', ' : 'Last modified '}{timeAgo(new Date(node.last_modified_at * 1000))}{/if}
      </p>
    </div>
  {/if}

  <!-- Persona (only when the extractor recorded one — never fabricated) -->
  {#if node.persona}
    <div class="detail-section">
      <p class="detail-modified-summary">Persona: <code class="persona-tag">{node.persona}</code></p>
    </div>
  {/if}

  <!-- Fields with clickable types -->
  {#if relationships.fields.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Fields ({relationships.fields.length})</summary>
      <ul class="detail-ref-list">
        {#each relationships.fields as f (f.id)}
          {@const fieldType = resolveFieldType(f, nodes, edges)}
          <li>
            <button class="detail-ref-link" onclick={() => onNavigate(f)} type="button">
              <span class="ref-type">{f.node_type}</span>
              <span class="field-name">{f.name}</span>
              {#if fieldType?.annotation}
                {#if fieldType.typeNode}
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span
                    class="field-type-annotation field-type-clickable"
                    role="button"
                    tabindex="0"
                    title="Navigate to type {fieldType.typeNode.name}"
                    onclick={(e) => { e.stopPropagation(); onNavigate(fieldType.typeNode); }}
                    onkeydown={(e) => { if (e.key === 'Enter') { e.stopPropagation(); onNavigate(fieldType.typeNode); } }}
                  >: {fieldType.annotation}</span>
                {:else}
                  <span class="field-type-annotation">: {fieldType.annotation}</span>
                {/if}
              {/if}
            </button>
          </li>
        {/each}
      </ul>
    </details>
  {/if}

  <!-- Implements (traits this type implements) -->
  {#if relationships.implements.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Implements ({relationships.implements.length})</summary>
      <ul class="detail-ref-list">
        {#each relationships.implements as impl (impl.id)}
          <li>
            <button class="detail-ref-link" onclick={() => onNavigate(impl)} type="button">
              <span class="ref-type">{impl.node_type}</span> {impl.name}
            </button>
          </li>
        {/each}
      </ul>
    </details>
  {:else}
    <details class="detail-collapsible">
      <summary class="detail-section-title">Implements</summary>
      <p class="detail-muted">none — this is a domain entity</p>
    </details>
  {/if}

  <!-- Used by: call sites grouped by crate/module -->
  {#if usedBy.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Used By ({usedBy.length})</summary>
      <ul class="detail-ref-list">
        {#each usedBy.slice(0, 15) as user (user.id)}
          <li>
            <button class="detail-ref-link" onclick={() => onNavigate(user)} type="button">
              <span class="ref-type">{user.node_type}</span> {user.name}
              {#if crateOf(user)}
                <span class="impl-location">{crateOf(user)}</span>
              {/if}
            </button>
          </li>
        {/each}
        {#if usedBy.length > 15}
          <li class="detail-more">+{usedBy.length - 15} more</li>
        {/if}
      </ul>
    </details>
  {/if}

  <!-- Story: creation/modification history -->
  <details class="detail-collapsible" open>
    <summary class="detail-section-title">Story</summary>
    <p class="detail-story story-chronological">
      {#if node.created_at}Created on {new Date(node.created_at * 1000).toLocaleDateString()}{:else}Created at unknown date{/if}{#if relationships.fields.length > 0}, {relationships.fields.length} field{relationships.fields.length !== 1 ? 's' : ''}{/if}{#if node.spec_path || relationships.governedBy}, governed by <code class="story-spec-ref">{node.spec_path ?? relationships.governedBy}</code>{/if}.
    </p>
    {#if node.doc_comment}
      <p class="detail-doc">{node.doc_comment}</p>
    {/if}
    {#if story}
      <p class="detail-story">{story}</p>
    {/if}
    <div class="story-provenance">
      {#if node.created_sha}
        <p class="detail-provenance">
          <span class="provenance-icon">&#x1f4dd;</span>
          Created in <code>{node.created_sha.slice(0, 7)}</code>{#if node.created_by} by <code class="persona-tag">{node.created_by}</code>{/if}
        </p>
      {/if}
      {#if node.last_modified_by}
        <p class="detail-provenance">
          <span class="provenance-icon">&#x270d;</span>
          Last modified by
          {#if onAgentProvenance}
            <button class="agent-ref" onclick={() => onAgentProvenance(node.last_modified_by)} type="button" title="Show agent provenance">{node.last_modified_by}</button>
          {:else}
            <code class="persona-tag">{node.last_modified_by}</code>
          {/if}
          {#if node.last_modified_at}, {timeAgo(new Date(node.last_modified_at * 1000))}{/if}
        </p>
      {/if}
      {#if node.last_modified_sha && node.last_modified_sha !== node.created_sha}
        <p class="detail-provenance">
          Commit <code>{node.last_modified_sha.slice(0, 7)}</code>
        </p>
      {/if}
      {#if node.churn_count_30d}
        <p class="detail-provenance">{node.churn_count_30d} modification{node.churn_count_30d !== 1 ? 's' : ''} in last 30 days</p>
      {/if}
    </div>
    {#if !story && !node.doc_comment && !node.created_sha && !node.last_modified_by && !node.created_at}
      <p class="detail-story detail-muted">No documentation or history available.</p>
    {/if}
  </details>

  <!-- Risk footer: churn / coupling / spec coverage / tests -->
  {#if risk}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Risk Summary</summary>
      <div class="risk-detail-rows">
        <div class="risk-row">
          <span class="risk-row-label">Churn</span>
          <div class="risk-bar-track">
            <div class="risk-bar-fill risk-bar-{risk.churnLabel}" style="width: {risk.churnPct}%"></div>
          </div>
          <span class="risk-row-value">{risk.churn}/30d</span>
        </div>
        <div class="risk-row">
          <span class="risk-row-label">Coupling</span>
          <span class="risk-row-value risk-coupling-{risk.couplingLabel}">{risk.couplingLabel} ({risk.coupling})</span>
        </div>
        <div class="risk-row">
          <span class="risk-row-label">Spec</span>
          <span class="risk-row-value">{#if risk.hasSpec}<span class="risk-badge risk-badge-good">covered</span>{#if risk.specConfidence !== 'high' && risk.specConfidence !== 'none'} <span class="risk-confidence">({risk.specConfidence})</span>{/if}{:else}<span class="risk-badge risk-badge-warn">none</span>{/if}</span>
        </div>
        <div class="risk-row">
          <span class="risk-row-label">Tests</span>
          <span class="risk-row-value">{risk.testLabel}</span>
        </div>
        {#if risk.complexity != null}
          <div class="risk-row">
            <span class="risk-row-label">Complexity</span>
            <div class="risk-bar-track">
              <div class="risk-bar-fill risk-bar-{risk.complexityLabel}" style="width: {risk.complexityPct}%"></div>
            </div>
            <span class="risk-row-value">{risk.complexity}</span>
          </div>
        {/if}
      </div>
    </details>
  {/if}
</div>
