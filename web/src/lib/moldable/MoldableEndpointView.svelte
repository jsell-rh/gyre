<script>
  /**
   * MoldableEndpointView — system-explorer.md §2 "Endpoint View".
   *
   * Tailored detail view for API endpoint graph nodes: route (method + path,
   * from the RoutesTo edge metadata the extractor emits), spec linkage,
   * handler location (file:line), request/response shapes, the call flow
   * diagram (auth → … → respond as the handler call chain), gates, and
   * test coverage count.
   */
  import './shared.css';
  import {
    computeRelationships,
    computeRisk,
    computeStory,
    computeEndpointMeta,
    computeEndpointFlow,
    computeEndpointShapes,
    computeEndpointGates,
    computeEndpointTestCount,
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
  let endpointMeta = $derived(computeEndpointMeta(node, nodes, edges));
  let flow = $derived(computeEndpointFlow(node, nodes, edges));
  let shapes = $derived(computeEndpointShapes(node, relationships, nodes, edges));
  let gates = $derived(computeEndpointGates(node));
  let testCount = $derived(computeEndpointTestCount(node, relationships, nodes, edges));

  function navigateToSpec() {
    const target = node.spec_path ?? relationships.governedBy;
    onNavigate({ id: target, name: target, node_type: 'spec' });
  }
</script>

<div class="moldable-view moldable-endpoint-view" data-moldable-view="endpoint">
  <!-- Route: method + path (from edge metadata, doc_comment, or node name) -->
  {#if endpointMeta}
    <div class="detail-section">
      <h4 class="detail-section-title">Route</h4>
      {#if endpointMeta.method || endpointMeta.path}
        <p class="detail-endpoint-method">
          {#if endpointMeta.method}<span class="http-method http-{endpointMeta.method.toLowerCase()}">{endpointMeta.method}</span>{/if}
          <code>{endpointMeta.path || node.qualified_name || node.name}</code>
        </p>
      {:else}
        <p class="detail-endpoint-method"><code>{node.qualified_name || node.name}</code></p>
      {/if}
    </div>
  {/if}

  <!-- Spec linkage (clickable → opens the spec inline) -->
  {#if node.spec_path || relationships.governedBy}
    <div class="detail-section">
      <h4 class="detail-section-title">Spec</h4>
      <button class="detail-spec-button" onclick={navigateToSpec} type="button" title="Open spec inline">
        {node.spec_path ?? relationships.governedBy}
      </button>
    </div>
  {/if}

  <!-- Handler (file:line) -->
  {#if (relationships.routesTo.length ?? 0) > 0 || (endpointMeta?.routesTo.length ?? 0) > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Handler</summary>
      <ul class="detail-ref-list">
        {#each (relationships.routesTo.length > 0 ? relationships.routesTo : endpointMeta.routesTo) as handler (handler.id)}
          <li>
            <button class="detail-ref-link" onclick={() => onNavigate(handler)} type="button">
              <span class="ref-type">{handler.node_type}</span> {handler.name}
              {#if handler.file_path}
                <span class="handler-location">{handler.file_path}{handler.line_start ? `:${handler.line_start}` : ''}</span>
              {/if}
            </button>
          </li>
        {/each}
      </ul>
    </details>
  {/if}

  <!-- Request / Response body shapes -->
  {#if shapes}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Request / Response</summary>
      <div class="endpoint-shapes">
        {#if shapes.params.length > 0}
          <div class="shape-row">
            <span class="shape-label">Params</span>
            <div class="shape-params">
              {#each shapes.params as param (param)}
                <code class="shape-param">{param}</code>
              {/each}
            </div>
          </div>
        {/if}
        {#if shapes.requestBody}
          <div class="shape-row">
            <span class="shape-label">Request</span>
            <code class="shape-type">{shapes.requestBody}</code>
          </div>
        {/if}
        {#if shapes.responseBody}
          <div class="shape-row">
            <span class="shape-label">Response</span>
            <code class="shape-type">{shapes.responseBody}</code>
          </div>
        {/if}
      </div>
    </details>
  {/if}

  <!-- Flow: call chain from the handler (auth → … → respond) -->
  {#if flow.length > 0}
    <details class="detail-collapsible" open>
      <summary class="detail-section-title">Flow ({flow.length})</summary>
      <ul class="detail-ref-list flow-chain">
        {#each flow as step (step.node.id)}
          <li class="flow-step" style="padding-left: {step.depth * 12}px">
            <span class="flow-arrow">{step.depth === 0 ? '&#9656;' : '&#8627;'}</span>
            <button class="detail-ref-link" onclick={() => onNavigate(step.node)} type="button">
              <span class="ref-type">{step.node.node_type}</span> {step.node.name}
            </button>
          </li>
        {/each}
      </ul>
    </details>
  {/if}

  <!-- Gates -->
  {#if gates.length > 0}
    <div class="detail-section">
      <h4 class="detail-section-title">Gates</h4>
      <div class="gate-list">
        {#each gates as gate (gate)}
          <span class="gate-badge">{gate}</span>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Tests -->
  <div class="detail-section">
    <h4 class="detail-section-title">Tests</h4>
    {#if relationships.testedBy.length > 0}
      <details class="detail-collapsible" open>
        <summary class="detail-section-title">{relationships.testedBy.length} test{relationships.testedBy.length !== 1 ? 's' : ''} covering this endpoint</summary>
        <ul class="detail-ref-list">
          {#each relationships.testedBy as testNode (testNode.id)}
            <li>
              <button class="detail-ref-link" onclick={() => onNavigate(testNode)} type="button">
                <span class="ref-type">test</span> {testNode.name}
              </button>
            </li>
          {/each}
        </ul>
      </details>
    {:else if testCount > 0}
      <p class="detail-test-count">{testCount} test{testCount !== 1 ? 's' : ''} {testCount !== 1 ? 'reach' : 'reaches'} this endpoint</p>
    {:else}
      <p class="detail-story detail-muted">No tests found for this endpoint.</p>
    {/if}
  </div>

  <!-- Doc comment -->
  {#if node.doc_comment}
    <div class="detail-section">
      <h4 class="detail-section-title">Documentation</h4>
      <p class="detail-doc">{node.doc_comment}</p>
    </div>
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
