<script>
  /**
   * MoldableTraitView — system-explorer.md §2 "Trait View".
   *
   * Tailored detail view for trait/interface graph nodes: spec linkage
   * (clickable → opens spec inline), crate location, methods with
   * signatures, implementations (with crate attribution), dependents with
   * call-site counts, story, and risk.
   */
  import './shared.css';
  import {
    computeRelationships,
    computeRisk,
    computeStory,
    computeMethodCallCounts,
    computeTraitDependents,
    extractMethodSignature,
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
  let methodCallCounts = $derived(computeMethodCallCounts(node, relationships, edges));
  // Dependents (spec: "who uses this trait, with call site counts") — every
  // caller of the trait or its methods, with its call-site count.
  let dependents = $derived(computeTraitDependents(node, relationships, edges, nodes));

  let crateName = $derived(node?.qualified_name ? node.qualified_name.split('::')[0] : null);

  function navigateToSpec() {
    const target = node.spec_path ?? relationships.governedBy;
    onNavigate({ id: target, name: target, node_type: 'spec' });
  }
</script>

<div class="moldable-view moldable-trait-view" data-moldable-view="trait">
  <!-- Spec linkage (clickable → opens the spec inline) -->
  {#if node.spec_path || relationships.governedBy}
    <div class="detail-section">
      <h4 class="detail-section-title">Spec</h4>
      <button class="detail-spec-button" onclick={navigateToSpec} type="button" title="Open spec inline">
        {node.spec_path ?? relationships.governedBy}
      </button>
    </div>
  {/if}

  <!-- Crate location -->
  {#if crateName}
    <div class="detail-section">
      <h4 class="detail-section-title">Crate</h4>
      <p class="detail-crate"><code>{crateName}</code></p>
    </div>
  {/if}

  <!-- Doc comment -->
  {#if node.doc_comment}
    <div class="detail-section">
      <h4 class="detail-section-title">Documentation</h4>
      <p class="detail-doc">{node.doc_comment}</p>
    </div>
  {/if}

  <!-- Methods with signatures + per-method call-site counts -->
  {#if relationships.methods.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Methods ({relationships.methods.length})</summary>
      <ul class="detail-ref-list method-signatures">
        {#each relationships.methods as method (method.id)}
          {@const sig = extractMethodSignature(method)}
          {@const calls = methodCallCounts.get(method.id) ?? 0}
          <li>
            <button class="detail-ref-link method-link" onclick={() => onNavigate(method)} type="button">
              <span class="method-sig-line">
                <span class="ref-type">fn</span>
                <span class="method-name">{method.name}</span>
                {#if sig.params !== null}
                  <span class="method-params">({sig.params})</span>
                {:else}
                  <span class="method-params">()</span>
                {/if}
                {#if sig.returnType}
                  <span class="method-return-arrow"> -&gt; </span>
                  <span class="method-return-type">{sig.returnType}</span>
                {/if}
              </span>
              <span class="call-count" class:call-count-zero={calls === 0} title="Call sites">{calls} call{calls !== 1 ? 's' : ''}</span>
            </button>
          </li>
        {/each}
      </ul>
    </details>
  {/if}

  <!-- Implementations (all impls with crate attribution) -->
  {#if relationships.implementedBy.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Implementations ({relationships.implementedBy.length})</summary>
      <ul class="detail-ref-list">
        {#each relationships.implementedBy as impl (impl.id)}
          <li>
            <button class="detail-ref-link" onclick={() => onNavigate(impl)} type="button">
              <span class="ref-type">{impl.node_type}</span> {impl.name}
              {#if impl.qualified_name}
                <span class="impl-location">{impl.qualified_name.split('::')[0]}</span>
              {:else if impl.file_path}
                <span class="impl-location">{impl.file_path.split('/').pop()}</span>
              {/if}
            </button>
          </li>
        {/each}
      </ul>
    </details>
  {/if}

  <!-- Dependents (who uses this trait, with call-site counts) -->
  {#if dependents.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Dependents ({dependents.length})</summary>
      <ul class="detail-ref-list">
        {#each dependents.slice(0, 15) as dep (dep.node.id)}
          <li>
            <button class="detail-ref-link" onclick={() => onNavigate(dep.node)} type="button">
              <span class="ref-type">{dep.node.node_type}</span> {dep.node.name}
              {#if dep.node.qualified_name}
                <span class="impl-location">{dep.node.qualified_name.split('::')[0]}</span>
              {/if}
              <span class="call-count" class:call-count-zero={dep.calls === 0} title="Call sites into this trait">{dep.calls} call{dep.calls !== 1 ? 's' : ''}</span>
            </button>
          </li>
        {/each}
        {#if dependents.length > 15}
          <li class="detail-more">+{dependents.length - 15} more</li>
        {/if}
      </ul>
    </details>
  {/if}

  <!-- Story -->
  <details class="detail-collapsible" open>
    <summary class="detail-section-title">Story</summary>
    {#if story}
      <p class="detail-story">{story}</p>
    {/if}
    <div class="story-provenance">
      {#if node.created_at}
        <p class="detail-provenance">Created {new Date(node.created_at * 1000).toLocaleDateString()}</p>
      {/if}
      {#if node.created_sha}
        <p class="detail-provenance">Created in <code>{node.created_sha.slice(0, 7)}</code></p>
      {/if}
      {#if node.last_modified_by}
        <p class="detail-provenance"><span class="provenance-icon">&#x270d;</span> Last modified by
          {#if onAgentProvenance}
            <button class="agent-ref" onclick={() => onAgentProvenance(node.last_modified_by)} type="button" title="Show agent provenance">{node.last_modified_by}</button>
          {:else}
            <code class="persona-tag">{node.last_modified_by}</code>
          {/if}
          {#if node.last_modified_at}, {timeAgo(new Date(node.last_modified_at * 1000))}{/if}
        </p>
      {/if}
      {#if node.last_modified_sha && node.last_modified_sha !== node.created_sha}
        <p class="detail-provenance">Commit <code>{node.last_modified_sha.slice(0, 7)}</code></p>
      {/if}
      {#if node.churn_count_30d}
        <p class="detail-provenance">{node.churn_count_30d} modification{node.churn_count_30d !== 1 ? 's' : ''} in last 30 days</p>
      {/if}
    </div>
    {#if !story && !node.created_sha && !node.last_modified_by}
      <p class="detail-story detail-muted">No history available.</p>
    {/if}
  </details>

  <!-- Risk -->
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
          <span class="risk-row-value">{#if risk.hasSpec}<span class="risk-badge risk-badge-good">covered</span>{:else}<span class="risk-badge risk-badge-warn">none</span>{/if}</span>
        </div>
        <div class="risk-row">
          <span class="risk-row-label">Tests</span>
          <span class="risk-row-value">{risk.testLabel}</span>
        </div>
      </div>
    </details>
  {/if}
</div>
