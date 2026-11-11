<script>
  import MoldableTypeView from './moldable/MoldableTypeView.svelte';
  import MoldableTraitView from './moldable/MoldableTraitView.svelte';
  import MoldableEndpointView from './moldable/MoldableEndpointView.svelte';
  import MoldableSpecView from './moldable/MoldableSpecView.svelte';
  import {
    computeRelationships,
    computeRisk,
    computeStory,
    moldableViewType as moldableViewTypeOf,
    timeAgo,
    visibilityBadge as visibilityBadgeOf,
  } from './moldable/model.js';

  let {
    node = null,
    nodes = [],
    edges = [],
    onClose = () => {},
    onNavigate = () => {},
    onTraceStepNavigate = null, // causal trace step click: receives the step ({nodeId, name, ...}); falls back to onNavigate
    onInteractiveQuery = () => {},
    lens = 'structural',
    traceSpans = [],
    onSpanSelect = () => {},
    onAgentProvenance = null,
    onEditSpec = null,
    repoId = null,
  } = $props();

  // ── Evaluative tab: per-node span list and stats ──
  let expandedSpanId = $state(null);

  // Filter trace spans to those touching the selected node
  let nodeSpans = $derived.by(() => {
    if (!node || !traceSpans?.length) return [];
    const nodeId = node.id;
    return traceSpans
      .filter(s => s.graph_node_id === nodeId)
      .sort((a, b) => (b.duration_us ?? 0) - (a.duration_us ?? 0));
  });

  // Compute aggregate stats for the evaluative tab
  let evalStats = $derived.by(() => {
    if (nodeSpans.length === 0) return null;
    const durations = nodeSpans.map(s => s.duration_us ?? 0).sort((a, b) => a - b);
    const total = durations.length;
    const errors = nodeSpans.filter(s => (s.status ?? '').toLowerCase() === 'error').length;

    const percentile = (arr, p) => {
      if (arr.length === 0) return 0;
      if (arr.length === 1) return arr[0];
      const idx = p * (arr.length - 1);
      const lo = Math.floor(idx);
      const hi = Math.ceil(idx);
      const frac = idx - lo;
      return arr[lo] * (1 - frac) + arr[hi] * frac;
    };

    const p50 = percentile(durations, 0.5);
    const p95 = percentile(durations, 0.95);
    const mean = durations.reduce((a, b) => a + b, 0) / total;
    const errorRate = total > 0 ? errors / total : 0;

    return { spanCount: total, p50, p95, meanDuration: mean, errorRate };
  });

  let showEvalTab = $derived(lens === 'evaluative' && nodeSpans.length > 0);

  // Format duration in microseconds to human-readable
  function formatDuration(us) {
    if (us < 1000) return `${Math.round(us)}\u00B5s`;
    if (us < 1_000_000) return `${(us / 1000).toFixed(1)}ms`;
    return `${(us / 1_000_000).toFixed(2)}s`;
  }

  // Format span timestamp
  function formatSpanTime(ts) {
    if (!ts) return '';
    const d = new Date(ts / 1000); // ts is in microseconds
    return d.toLocaleTimeString();
  }

  // Compute relationships for the selected node (shared moldable model —
  // moldable/model.js is the single implementation used by the four
  // Moldable*View components and their tests too).
  let relationships = $derived(computeRelationships(node, nodes, edges));

  let nodeTypeLabel = $derived.by(() => {
    if (!node) return '';
    switch (node.node_type) {
      case 'type': return 'Type';
      case 'interface': return 'Interface / Trait';
      case 'function': return 'Function';
      case 'endpoint': return 'Endpoint';
      case 'module': return 'Module';
      case 'package': return 'Package';
      case 'component': return 'Component';
      case 'table': return 'Table';
      case 'constant': return 'Constant';
      case 'spec': return 'Spec';
      default: return node.node_type ?? 'Unknown';
    }
  });

  // Determine which moldable view to render based on node type.
  // The routing table lives in moldable/model.js (shared with the components
  // and tests); the view itself renders in the Moldable*View components.
  let moldableViewType = $derived(node ? moldableViewTypeOf(node.node_type) : null);

  let moldableViewLabel = $derived.by(() => {
    switch (moldableViewType) {
      case 'type': return 'Type View';
      case 'trait': return 'Trait View';
      case 'endpoint': return 'Endpoint View';
      case 'spec': return 'Spec View';
      default: return null;
    }
  });

  let moldableViewDescription = $derived.by(() => {
    switch (moldableViewType) {
      case 'type': return 'Fields, traits, consumers, and risk profile';
      case 'trait': return 'Methods, implementors, and dependents';
      case 'endpoint': return 'Route, handler flow, request/response shapes, and gates';
      case 'spec': return 'Governed nodes, implementation completeness, and coverage';
      default: return null;
    }
  });

  let visibilityBadge = $derived(node ? visibilityBadgeOf(node.visibility) : '');

  // Story + risk for the generic (non-moldable) view; each moldable view
  // computes its own from the same shared model functions.
  let story = $derived(computeStory(node, relationships, edges));
  let nodeRisk = $derived(computeRisk(node, relationships));

  // Compute produced_by persona from last_modified_by or produced_by edge
  let producedBy = $derived.by(() => {
    if (!node) return null;
    if (node.last_modified_by) return node.last_modified_by;
    if (node.produced_by) return node.produced_by;
    // Check for produced_by edge
    for (const e of edges) {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if ((et === 'produced_by' || et === 'last_modified_by') && src === node.id) {
        const tgtNode = nodes.find(n => n.id === tgt);
        return tgtNode?.name ?? tgt;
      }
    }
    return null;
  });

  function handleNodeClick(targetNode) {
    if (targetNode) {
      onNavigate(targetNode);
    }
  }
</script>

{#if !node}
  <div class="detail-panel" role="complementary" aria-label="Explorer flows">
    <div class="detail-header">
      <div class="detail-title-row">
        <span class="detail-type-badge">Explorer</span>
        <button class="detail-close" onclick={onClose} aria-label="Close" type="button">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
      <h3 class="detail-name">Common Flows</h3>
    </div>
    <div class="detail-body">
      <div class="detail-section">
        <h4 class="detail-section-title">Predefined Traces</h4>
        <p class="detail-story">Click a flow to trace its path through the architecture graph.</p>
        <div class="common-flows-list">
          <button
            class="common-flow-btn"
            onclick={() => onInteractiveQuery({ type: 'blast_radius', from_function: 'spawn_agent', depth: 4, label: 'Agent spawn flow' })}
            type="button"
          >
            <span class="common-flow-icon">&#9654;</span>
            <div class="common-flow-info">
              <span class="common-flow-name">Agent spawn flow</span>
              <span class="common-flow-desc">Trace from spawn_agent through containers, gates, and sessions</span>
            </div>
          </button>
          <button
            class="common-flow-btn"
            onclick={() => onInteractiveQuery({ type: 'blast_radius', from_function: 'approve_spec', depth: 4, label: 'Spec approval flow' })}
            type="button"
          >
            <span class="common-flow-icon">&#9654;</span>
            <div class="common-flow-info">
              <span class="common-flow-name">Spec approval flow</span>
              <span class="common-flow-desc">Trace from approve_spec through lifecycle, agents, and implementation</span>
            </div>
          </button>
        </div>
      </div>
    </div>
  </div>
{:else if node}
  <div class="detail-panel" role="complementary" aria-label="Node details">
    <div class="detail-header">
      <div class="detail-title-row">
        <span class="detail-type-badge">{nodeTypeLabel}</span>
        {#if visibilityBadge}
          <span class="detail-vis-badge">[{visibilityBadge}]</span>
        {/if}
        <button class="detail-close" onclick={onClose} aria-label="Close" type="button">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="14" height="14"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
        </button>
      </div>
      <h3 class="detail-name">{node.name ?? node.qualified_name ?? 'Unknown'}</h3>
      {#if node.qualified_name && node.qualified_name !== node.name}
        <p class="detail-qualified">{node.qualified_name}</p>
      {/if}
    </div>

    <!-- Moldable View Banner -->
    {#if moldableViewLabel}
      <div class="moldable-view-banner">
        <div class="moldable-view-indicator">
          <span class="moldable-view-icon">
            {#if moldableViewType === 'type'}
              <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" width="14" height="14"><rect x="2" y="2" width="12" height="12" rx="2"/><line x1="2" y1="6" x2="14" y2="6"/><line x1="2" y1="10" x2="14" y2="10"/></svg>
            {:else if moldableViewType === 'trait'}
              <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" width="14" height="14"><circle cx="8" cy="4" r="2.5"/><path d="M3 14 L8 8 L13 14"/></svg>
            {:else if moldableViewType === 'endpoint'}
              <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" width="14" height="14"><path d="M2 8h12M10 4l4 4-4 4"/></svg>
            {:else if moldableViewType === 'spec'}
              <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" width="14" height="14"><path d="M4 2h6l4 4v8a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V3a1 1 0 0 1 1-1z"/><path d="M10 2v4h4"/></svg>
            {/if}
          </span>
          <div class="moldable-view-text">
            <span class="moldable-view-label">{moldableViewLabel}</span>
            <span class="moldable-view-desc">{moldableViewDescription}</span>
          </div>
        </div>
      </div>
    {/if}

    <div class="detail-body">
      <!-- Location (all node types) -->
      {#if node.file_path}
        <div class="detail-section">
          <h4 class="detail-section-title">Location</h4>
          <p class="detail-file">
            <code>{node.file_path}{node.line_start ? `:${node.line_start}` : ''}</code>
          </p>
        </div>
      {/if}

      <!-- Contained in (all node types) -->
      {#if relationships.containedIn}
        <div class="detail-section">
          <h4 class="detail-section-title">Contained In</h4>
          <button class="detail-ref-link" onclick={() => handleNodeClick(relationships.containedIn)} type="button">
            <span class="ref-type">{relationships.containedIn.node_type}</span>
            {relationships.containedIn.name}
          </button>
        </div>
      {/if}

      <!-- Causal Flow Trace: step-by-step path when a trace is active -->
      {#if node._traceSteps?.length > 0}
        <details class="detail-collapsible" open>
          <summary class="detail-section-title">
            Causal Flow Trace
            <span class="detail-badge">{node._traceSteps.length} steps</span>
          </summary>
          <div class="trace-step-list">
            {#each node._traceSteps as step}
              <button
                class="trace-step-item"
                class:trace-current={step.nodeId === node.id}
                onclick={() => {
                  // Pan/zoom the canvas to the step's node when the host wires
                  // the trace-specific callback; otherwise navigate within the
                  // detail panel as before.
                  if (onTraceStepNavigate) onTraceStepNavigate(step);
                  else onNavigate(nodes.find(n => n.id === step.nodeId) ?? { id: step.nodeId, name: step.name });
                }}
                type="button"
              >
                <span class="trace-step-num">{step.step}</span>
                <div class="trace-step-info">
                  <span class="trace-step-name">{step.name}</span>
                  <span class="trace-step-type">{step.type}</span>
                  {#if step.specPath}
                    <span class="trace-step-spec" title="Governed by spec">{step.specPath}</span>
                  {/if}
                </div>
                {#if step.outEdges.length > 0}
                  <span class="trace-step-arrow" title="Calls to step {step.outEdges[0].toStep}">&#8594;</span>
                {/if}
              </button>
            {/each}
          </div>
        </details>
      {/if}

      <!-- ============================================ -->
      <!-- TYPE VIEW: fields / implements / used-by / story / risk -->
      <!-- ============================================ -->
      <!-- ============================================ -->
      <!-- MOLDABLE VIEWS (system-explorer.md §2) — routed -->
      <!-- by node_type to the tailored view component.    -->
      <!-- ============================================ -->
      {#if moldableViewType === 'type'}
        <MoldableTypeView {node} {nodes} {edges} onNavigate={handleNodeClick} {onAgentProvenance} />
      {:else if moldableViewType === 'trait'}
        <MoldableTraitView {node} {nodes} {edges} onNavigate={handleNodeClick} {onAgentProvenance} />
      {:else if moldableViewType === 'endpoint'}
        <MoldableEndpointView {node} {nodes} {edges} onNavigate={handleNodeClick} {onAgentProvenance} />
      {:else if moldableViewType === 'spec'}
        <MoldableSpecView {node} {nodes} {edges} onNavigate={handleNodeClick} {repoId} {onEditSpec} />
      {:else if node.node_type === 'edge'}
        <div class="detail-section">
          <h4 class="detail-section-title">Relationship</h4>
          <p class="detail-edge-type"><code>{(node.edge_type ?? '').replace('_', ' ')}</code></p>
          {#if node.source_node}
            <div class="detail-edge-endpoint">
              <span class="detail-muted">From:</span>
              <button class="detail-ref-link" onclick={() => handleNodeClick(node.source_node)} type="button">
                <span class="ref-type">{node.source_node.node_type}</span> {node.source_node.name}
              </button>
              {#if node.source_node.file_path}
                <code class="detail-muted detail-small">{node.source_node.file_path}{node.source_node.line_start ? `:${node.source_node.line_start}` : ''}</code>
              {/if}
            </div>
          {/if}
          {#if node.target_node}
            <div class="detail-edge-endpoint">
              <span class="detail-muted">To:</span>
              <button class="detail-ref-link" onclick={() => handleNodeClick(node.target_node)} type="button">
                <span class="ref-type">{node.target_node.node_type}</span> {node.target_node.name}
              </button>
              {#if node.target_node.file_path}
                <code class="detail-muted detail-small">{node.target_node.file_path}{node.target_node.line_start ? `:${node.target_node.line_start}` : ''}</code>
              {/if}
            </div>
          {/if}
        </div>

      <!-- ============================================ -->
      <!-- SPAN VIEW: OTLP trace span detail (from evaluative particles) -->
      <!-- ============================================ -->
      {:else if node.node_type === 'span'}
        <div class="detail-section">
          <h4 class="detail-section-title">Span</h4>
          <div class="span-detail-grid">
            <span class="detail-muted">Operation</span>
            <code>{node.name}</code>
            {#if node.service_name}
              <span class="detail-muted">Service</span>
              <code>{node.service_name}</code>
            {/if}
            <span class="detail-muted">Duration</span>
            <code>{node.duration_us != null ? (node.duration_us > 1000 ? `${(node.duration_us / 1000).toFixed(1)}ms` : `${node.duration_us}\u00B5s`) : '?'}</code>
            <span class="detail-muted">Status</span>
            <code class:span-error={(node.status ?? '').toLowerCase() === 'error'}>{node.status}</code>
            {#if node.span_id}
              <span class="detail-muted">Span ID</span>
              <code class="detail-small">{node.span_id}</code>
            {/if}
          </div>
        </div>
        {#if node.attributes && Object.keys(node.attributes).length > 0}
          <div class="detail-section">
            <details open>
              <summary class="detail-section-title">Attributes ({Object.keys(node.attributes).length})</summary>
              <div class="span-attributes">
                {#each Object.entries(node.attributes) as [key, value]}
                  <div class="span-attr-row">
                    <span class="span-attr-key">{key}</span>
                    <span class="span-attr-value">{value}</span>
                  </div>
                {/each}
              </div>
            </details>
          </div>
        {/if}
        {#if node.input_summary}
          <div class="detail-section">
            <h4 class="detail-section-title">Input</h4>
            <pre class="span-io-summary">{node.input_summary}</pre>
          </div>
        {/if}
        {#if node.output_summary}
          <div class="detail-section">
            <h4 class="detail-section-title">Output</h4>
            <pre class="span-io-summary">{node.output_summary}</pre>
          </div>
        {/if}

      <!-- ============================================ -->
      <!-- GENERIC VIEW: function / module / package / constant / other -->
      <!-- ============================================ -->
      {:else}
        <!-- Doc comment -->
        {#if node.doc_comment}
          <div class="detail-section">
            <h4 class="detail-section-title">Documentation</h4>
            <p class="detail-doc">{node.doc_comment}</p>
          </div>
        {/if}

        <!-- Spec linkage -->
        {#if node.spec_path || relationships.governedBy}
          <div class="detail-section">
            <h4 class="detail-section-title">Spec</h4>
            <button class="detail-spec-button" onclick={() => onNavigate({ id: node.spec_path ?? relationships.governedBy, name: node.spec_path ?? relationships.governedBy, node_type: 'spec' })} type="button">
              {node.spec_path ?? relationships.governedBy}
            </button>
          </div>
        {/if}

        <!-- Contains (children) -->
        {#if relationships.contains.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Contains ({relationships.contains.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.contains.slice(0, 15) as c}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(c)} type="button">
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

        <!-- Implements -->
        {#if relationships.implements.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Implements</summary>
            <ul class="detail-ref-list">
              {#each relationships.implements as impl}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(impl)} type="button">
                    <span class="ref-type">{impl.node_type}</span> {impl.name}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <!-- Implemented by -->
        {#if relationships.implementedBy.length > 0}
          <details class="detail-collapsible">
            <summary class="detail-section-title">Implemented By ({relationships.implementedBy.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.implementedBy as impl}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(impl)} type="button">
                    <span class="ref-type">{impl.node_type}</span> {impl.name}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <!-- Called by -->
        {#if relationships.calledBy.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Called By ({relationships.calledBy.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.calledBy.slice(0, 10) as caller}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(caller)} type="button">
                    <span class="ref-type">{caller.node_type}</span> {caller.name}
                  </button>
                </li>
              {/each}
              {#if relationships.calledBy.length > 10}
                <li class="detail-more">+{relationships.calledBy.length - 10} more</li>
              {/if}
            </ul>
          </details>
        {/if}

        <!-- Calls -->
        {#if relationships.calls.length > 0}
          <details class="detail-collapsible">
            <summary class="detail-section-title">Calls ({relationships.calls.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.calls.slice(0, 10) as callee}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(callee)} type="button">
                    <span class="ref-type">{callee.node_type}</span> {callee.name}
                  </button>
                </li>
              {/each}
              {#if relationships.calls.length > 10}
                <li class="detail-more">+{relationships.calls.length - 10} more</li>
              {/if}
            </ul>
          </details>
        {/if}

        <!-- Story -->
        <details class="detail-collapsible" open>
          <summary class="detail-section-title">Story</summary>
          {#if node.doc_comment}
            <p class="detail-doc">{node.doc_comment}</p>
          {/if}
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
              <p class="detail-provenance">Last modified by <code>{node.last_modified_by}</code></p>
            {/if}
            {#if node.last_modified_sha && node.last_modified_sha !== node.created_sha}
              <p class="detail-provenance">Last modified in <code>{node.last_modified_sha.slice(0, 7)}</code>{#if node.last_modified_at}, {timeAgo(new Date(node.last_modified_at * 1000))}{/if}</p>
            {/if}
            {#if node.churn_count_30d}
              <p class="detail-provenance">{node.churn_count_30d} modification{node.churn_count_30d !== 1 ? 's' : ''} in last 30 days</p>
            {/if}
          </div>
          {#if !story && !node.doc_comment && !node.created_sha && !node.last_modified_by}
            <p class="detail-story detail-muted">No documentation or history available.</p>
          {/if}
        </details>

        <!-- Risk Summary (function/generic) -->
        {#if nodeRisk}
          <details class="detail-collapsible">
            <summary class="detail-section-title">Risk Summary</summary>
            <div class="risk-detail-rows">
              <div class="risk-row">
                <span class="risk-row-label">Churn</span>
                <div class="risk-bar-track">
                  <div class="risk-bar-fill risk-bar-{nodeRisk.churnLabel}" style="width: {nodeRisk.churnPct}%"></div>
                </div>
                <span class="risk-row-value">{nodeRisk.churn}/30d</span>
              </div>
              <div class="risk-row">
                <span class="risk-row-label">Coupling</span>
                <span class="risk-row-value risk-coupling-{nodeRisk.couplingLabel}">{nodeRisk.couplingLabel} ({nodeRisk.coupling})</span>
              </div>
              <div class="risk-row">
                <span class="risk-row-label">Spec</span>
                <span class="risk-row-value">{#if nodeRisk.hasSpec}<span class="risk-badge risk-badge-good">covered</span>{:else}<span class="risk-badge risk-badge-warn">none</span>{/if}</span>
              </div>
              <div class="risk-row">
                <span class="risk-row-label">Tests</span>
                <span class="risk-row-value">{nodeRisk.testLabel}</span>
              </div>
              {#if nodeRisk.complexity != null}
                <div class="risk-row">
                  <span class="risk-row-label">Complexity</span>
                  <div class="risk-bar-track">
                    <div class="risk-bar-fill risk-bar-{nodeRisk.complexityLabel}" style="width: {nodeRisk.complexityPct}%"></div>
                  </div>
                  <span class="risk-row-value">{nodeRisk.complexity}</span>
                </div>
              {/if}
            </div>
          </details>
        {/if}
      {/if}

      <!-- ============================================ -->
      <!-- EVALUATIVE TAB: per-node span list (all node types) -->
      <!-- ============================================ -->
      {#if showEvalTab}
        <details class="detail-collapsible" open>
          <summary class="detail-section-title">
            Evaluative
            <span class="detail-badge">{evalStats?.spanCount ?? 0} spans</span>
          </summary>

          <!-- Aggregate stats -->
          {#if evalStats}
            <div class="eval-stats-summary">
              <span class="eval-stat" title="Median duration">
                <span class="eval-stat-label">p50</span>
                <span class="eval-stat-value">{formatDuration(evalStats.p50)}</span>
              </span>
              <span class="eval-stat" title="95th percentile duration">
                <span class="eval-stat-label">p95</span>
                <span class="eval-stat-value">{formatDuration(evalStats.p95)}</span>
              </span>
              <span class="eval-stat" title="Error rate">
                <span class="eval-stat-label">Errors</span>
                <span class="eval-stat-value" class:eval-error-rate={evalStats.errorRate > 0}>{Math.round(evalStats.errorRate * 100)}%</span>
              </span>
              <span class="eval-stat" title="Mean duration">
                <span class="eval-stat-label">Mean</span>
                <span class="eval-stat-value">{formatDuration(evalStats.meanDuration)}</span>
              </span>
            </div>
          {/if}

          <!-- Span list sorted by duration descending -->
          <div class="eval-span-list">
            {#each nodeSpans as span (span.span_id)}
              <button
                class="eval-span-row"
                class:eval-span-error={(span.status ?? '').toLowerCase() === 'error'}
                class:eval-span-expanded={expandedSpanId === span.span_id}
                onclick={() => {
                  onSpanSelect(span);
                  expandedSpanId = expandedSpanId === span.span_id ? null : span.span_id;
                }}
                type="button"
              >
                <div class="eval-span-header">
                  <span class="eval-span-name" title={span.operation_name}>{span.operation_name ?? 'unknown'}</span>
                  <span class="eval-span-duration">{formatDuration(span.duration_us ?? 0)}</span>
                  <span class="eval-span-status" class:eval-status-ok={(span.status ?? '').toLowerCase() !== 'error'} class:eval-status-error={(span.status ?? '').toLowerCase() === 'error'}>
                    {((span.status ?? '').toLowerCase() === 'error') ? 'ERR' : 'OK'}
                  </span>
                </div>

                <!-- Expanded detail -->
                {#if expandedSpanId === span.span_id}
                  <div class="eval-span-detail">
                    {#if span.start_time}
                      <div class="eval-detail-row">
                        <span class="eval-detail-label">Time</span>
                        <span class="eval-detail-value">{formatSpanTime(span.start_time)}</span>
                      </div>
                    {/if}
                    {#if span.span_id}
                      <div class="eval-detail-row">
                        <span class="eval-detail-label">Span ID</span>
                        <span class="eval-detail-value"><code>{span.span_id}</code></span>
                      </div>
                    {/if}
                    {#if span.parent_span_id}
                      <div class="eval-detail-row">
                        <span class="eval-detail-label">Parent</span>
                        <span class="eval-detail-value"><code>{span.parent_span_id}</code></span>
                      </div>
                    {/if}
                    {#if span.attributes && Object.keys(span.attributes).length > 0}
                      <div class="eval-detail-row">
                        <span class="eval-detail-label">Attributes</span>
                        <div class="eval-attributes">
                          {#each Object.entries(span.attributes) as [key, val]}
                            <div class="eval-attr">
                              <span class="eval-attr-key">{key}</span>
                              <span class="eval-attr-val">{typeof val === 'object' ? JSON.stringify(val) : String(val)}</span>
                            </div>
                          {/each}
                        </div>
                      </div>
                    {/if}
                    {#if span.input_summary}
                      <div class="eval-detail-row">
                        <span class="eval-detail-label">Input</span>
                        <span class="eval-detail-value eval-detail-mono">{span.input_summary}</span>
                      </div>
                    {/if}
                    {#if span.output_summary}
                      <div class="eval-detail-row">
                        <span class="eval-detail-label">Output</span>
                        <span class="eval-detail-value eval-detail-mono">{span.output_summary}</span>
                      </div>
                    {/if}
                  </div>
                {/if}
              </button>
            {/each}
          </div>
        </details>
      {/if}

      <!-- ============================================ -->
      <!-- SHARED SECTIONS (all node types) -->
      <!-- ============================================ -->

      <!-- Risk Assessment -->
      {#if node.complexity != null || node.churn_count_30d || node.test_coverage != null}
        <div class="detail-section">
          <h4 class="detail-section-title">Risk Assessment</h4>
          <div class="risk-assessment">
            {#if (node.complexity ?? 0) > 20 && (node.test_coverage ?? 1) < 0.5}
              <p class="risk-item risk-high">High complexity ({node.complexity}) with low test coverage ({Math.round((node.test_coverage ?? 0) * 100)}%) — consider adding tests</p>
            {:else if (node.complexity ?? 0) > 30}
              <p class="risk-item risk-medium">High complexity ({node.complexity}) — consider refactoring</p>
            {:else if (node.churn_count_30d ?? 0) > 10 && relationships.calledBy.length > 5}
              <p class="risk-item risk-medium">High churn ({node.churn_count_30d}/30d) with many dependents ({relationships.calledBy.length} callers)</p>
            {:else if (node.test_coverage ?? 1) < 0.3 && node.node_type === 'function'}
              <p class="risk-item risk-medium">Low test coverage ({Math.round((node.test_coverage ?? 0) * 100)}%)</p>
            {:else}
              <p class="risk-item risk-low">Healthy — stable metrics</p>
            {/if}
          </div>
        </div>
      {/if}

      <!-- Metrics -->
      <div class="detail-section">
        <h4 class="detail-section-title">Metrics</h4>
        <div class="detail-metrics">
          {#if node.complexity != null}
            <span class="metric" title="Cyclomatic complexity">
              <span class="metric-label">Complexity</span>
              <span class="metric-value">{node.complexity}</span>
            </span>
          {/if}
          {#if node.test_node}
            <span class="metric test-node" title="Test function">
              <span class="metric-label">Test</span>
              <span class="metric-value">Yes</span>
            </span>
          {/if}
          {#if node.test_coverage != null}
            <span class="metric" title="Test coverage">
              <span class="metric-label">Coverage</span>
              <span class="metric-value">{Math.round((node.test_coverage ?? 0) * 100)}%</span>
            </span>
          {/if}
          {#if node.churn_count_30d}
            <span class="metric" title="Changes in last 30 days">
              <span class="metric-label">Churn/30d</span>
              <span class="metric-value">{node.churn_count_30d}</span>
            </span>
          {/if}
          <span class="metric" title="Incoming call edges">
            <span class="metric-label">Callers</span>
            <span class="metric-value">{relationships.calledBy.length}</span>
          </span>
          <span class="metric" title="Outgoing call edges">
            <span class="metric-label">Calls</span>
            <span class="metric-value">{relationships.calls.length}</span>
          </span>
        </div>
      </div>

      <!-- Produced by persona -->
      {#if producedBy}
        <div class="detail-section">
          <h4 class="detail-section-title">Produced By</h4>
          <p class="detail-persona"><code>{producedBy}</code></p>
        </div>
      {/if}

      <!-- Provenance -->
      {#if node.last_modified_by || node.created_sha}
        <details class="detail-collapsible">
          <summary class="detail-section-title">Provenance</summary>
          {#if node.last_modified_by}
            <p class="detail-provenance">Last modified by <code>{node.last_modified_by}</code></p>
          {/if}
          {#if node.last_modified_at}
            <p class="detail-provenance">Modified: {new Date(node.last_modified_at * 1000).toLocaleDateString()}</p>
          {/if}
          {#if node.created_sha}
            <p class="detail-provenance">Created in <code>{node.created_sha.slice(0, 7)}</code></p>
          {/if}
          {#if node.first_seen_at}
            <p class="detail-provenance">First seen: {new Date(node.first_seen_at * 1000).toLocaleDateString()}</p>
          {/if}
        </details>
      {/if}

      <!-- Spec Coverage -->
      {#if node.spec_path}
        {@const specNodes = nodes.filter(n => n.spec_path === node.spec_path && !n.deleted_at)}
        {#if specNodes.length > 0}
          <details class="detail-collapsible">
            <summary class="detail-section-title">Spec Coverage</summary>
            <p class="spec-completeness">
              <strong>{specNodes.length}</strong> node{specNodes.length !== 1 ? 's' : ''} governed by <code>{node.spec_path}</code>
            </p>
            <ul class="detail-ref-list">
              {#each specNodes.slice(0, 8) as sn}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(sn)} type="button">
                    <span class="ref-type">{sn.node_type}</span> {sn.name}
                    <span class="spec-check">✓</span>
                  </button>
                </li>
              {/each}
              {#if specNodes.length > 8}
                <li class="detail-more">+{specNodes.length - 8} more</li>
              {/if}
            </ul>
          </details>
        {/if}
      {/if}
    </div>
  </div>
{/if}

<style>
  .detail-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow-y: auto;
    background: var(--color-surface);
    border-left: 1px solid var(--color-border);
    min-width: 280px;
    max-width: 360px;
  }

  .detail-header {
    padding: var(--space-3) var(--space-4);
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-elevated);
  }

  .detail-title-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
  }

  .detail-type-badge {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-primary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .detail-vis-badge {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
  }

  .detail-close {
    margin-left: auto;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--color-text-muted);
    cursor: pointer;
  }

  .detail-close:hover {
    background: var(--color-surface);
    color: var(--color-text);
  }

  .detail-name {
    font-size: var(--text-base);
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--color-text);
    margin: 0;
    word-break: break-all;
  }

  .detail-qualified {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    font-family: var(--font-mono);
    margin: var(--space-1) 0 0;
    word-break: break-all;
  }

  .detail-body {
    padding: var(--space-3) var(--space-4);
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
  }

  .detail-section {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .detail-section-title {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin: 0;
  }

  .detail-file code {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-link);
    word-break: break-all;
  }

  .detail-doc {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    line-height: 1.5;
    margin: 0;
  }

  .detail-spec {
    font-size: var(--text-sm);
    color: var(--color-link);
    font-family: var(--font-mono);
    margin: 0;
  }

  .detail-provenance {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
    line-height: 1.6;
  }

  .detail-provenance code {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: color-mix(in srgb, var(--color-text) 8%, transparent);
    padding: 1px 4px;
    border-radius: 3px;
  }

  .detail-ref-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .detail-ref-link {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    background: transparent;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--color-link);
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    cursor: pointer;
    text-align: left;
    width: 100%;
    transition: background var(--transition-fast);
  }

  .detail-ref-link:hover {
    background: var(--color-surface-elevated);
  }

  .ref-type {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--color-text-muted);
    letter-spacing: 0.03em;
    flex-shrink: 0;
    min-width: 48px;
  }

  .detail-more {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    padding: var(--space-1) var(--space-2);
    font-style: italic;
  }

  .detail-metrics {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .metric {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    min-width: 56px;
  }

  .metric-label {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--color-text-muted);
    letter-spacing: 0.03em;
  }

  .metric-value {
    font-size: var(--text-sm);
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--color-text);
  }

  .metric.test-node {
    border-color: var(--color-success);
    background: color-mix(in srgb, var(--color-success) 10%, transparent);
  }

  .risk-assessment { margin: 0; }
  .risk-item {
    font-size: var(--text-xs); line-height: 1.5; margin: 0;
    padding: var(--space-1) var(--space-2); border-radius: var(--radius-sm);
  }
  .risk-high { background: color-mix(in srgb, #ef4444 12%, transparent); color: #fca5a5; border-left: 3px solid #ef4444; }
  .risk-medium { background: color-mix(in srgb, #f59e0b 12%, transparent); color: #fde68a; border-left: 3px solid #f59e0b; }
  .risk-low { background: color-mix(in srgb, #22c55e 10%, transparent); color: #bbf7d0; border-left: 3px solid #22c55e; }

  .spec-completeness {
    font-size: var(--text-xs); color: var(--color-text-secondary); margin: 0;
  }
  .spec-completeness code {
    font-family: var(--font-mono); font-size: var(--text-xs);
    background: color-mix(in srgb, var(--color-text) 8%, transparent);
    padding: 1px 4px; border-radius: 3px;
  }
  .spec-check { color: var(--color-success); margin-left: auto; font-size: 12px; }

  .detail-collapsible {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .detail-collapsible > summary {
    cursor: pointer;
    user-select: none;
    list-style: none;
    display: flex;
    align-items: center;
    gap: var(--space-1);
  }

  .detail-collapsible > summary::-webkit-details-marker { display: none; }

  .detail-collapsible > summary::before {
    content: '';
    display: inline-block;
    width: 0;
    height: 0;
    border-left: 5px solid var(--color-text-muted);
    border-top: 4px solid transparent;
    border-bottom: 4px solid transparent;
    transition: transform var(--transition-fast, 0.15s);
    flex-shrink: 0;
  }

  .detail-collapsible[open] > summary::before {
    transform: rotate(90deg);
  }

  .detail-spec-link {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
  }

  .detail-muted {
    color: var(--color-text-muted);
    font-style: italic;
  }

  /* Clickable spec link button */
  .detail-spec-button {
    display: inline-block;
    font-size: var(--text-sm);
    color: var(--color-link);
    font-family: var(--font-mono);
    background: transparent;
    border: none;
    cursor: pointer;
    padding: var(--space-1) 0;
    text-align: left;
    text-decoration: underline;
    text-decoration-style: dotted;
    text-underline-offset: 2px;
  }

  .detail-spec-button:hover {
    color: var(--color-primary);
    text-decoration-style: solid;
  }

  
  
  /* Risk summary grid for type view */
  .risk-summary-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-1);
  }

  .risk-metric {
    display: flex;
    flex-direction: column;
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
  }

  .risk-metric-label {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--color-text-muted);
    letter-spacing: 0.03em;
  }

  .risk-metric-value {
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--color-text);
  }

  .risk-coupling-high { color: #fca5a5; }
  .risk-coupling-medium { color: #fde68a; }
  .risk-coupling-low { color: #bbf7d0; }

  /* Story provenance section */
  .story-provenance {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-top: var(--space-1);
  }

  .completeness-stat-value {
    font-size: var(--text-sm);
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--color-text);
  }

  .completeness-stat-label {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--color-text-muted);
    letter-spacing: 0.03em;
  }

  .spec-type-group-label {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--color-text-secondary);
    letter-spacing: 0.03em;
    margin: var(--space-1) 0 0;
    padding-left: var(--space-2);
  }

  @media (prefers-reduced-motion: reduce) {
    .detail-ref-link { transition: none; }
    .detail-collapsible > summary::before { transition: none; }
    .completeness-fill, .completeness-tested-fill { transition: none; }
  }

  /* Edge view */
  .detail-edge-type { font-size: 14px; margin-bottom: 8px; }
  .detail-edge-endpoint { display: flex; flex-direction: column; gap: 2px; margin-bottom: 8px; }
  .detail-small { font-size: 11px; }

  /* Span view */
  .span-detail-grid { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; font-size: 13px; }
  .span-detail-grid code { word-break: break-all; }
  .span-error { color: #ef4444; font-weight: 600; }
  .span-attributes { display: flex; flex-direction: column; gap: 2px; font-size: 12px; }
  .span-attr-row { display: flex; gap: 8px; padding: 2px 0; border-bottom: 1px solid var(--color-border); }
  .span-attr-key { color: var(--color-text-muted); min-width: 80px; flex-shrink: 0; font-family: 'SF Mono', Menlo, monospace; }
  .span-attr-value { color: var(--color-text); word-break: break-all; font-family: 'SF Mono', Menlo, monospace; }
  .span-io-summary { font-size: 12px; font-family: 'SF Mono', Menlo, monospace; white-space: pre-wrap; word-break: break-all; background: rgba(0,0,0,0.2); padding: 8px; border-radius: 6px; max-height: 120px; overflow-y: auto; }

  /* Risk detail rows with visual bars */
  .risk-detail-rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .risk-row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 2px 0;
  }

  .risk-row-label {
    font-size: 9px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--color-text-muted);
    letter-spacing: 0.03em;
    min-width: 56px;
    flex-shrink: 0;
  }

  .risk-row-value {
    font-size: var(--text-xs);
    font-weight: 600;
    font-family: var(--font-mono);
    color: var(--color-text);
    flex-shrink: 0;
    min-width: 40px;
    text-align: right;
  }

  .risk-bar-track {
    flex: 1;
    height: 6px;
    background: color-mix(in srgb, var(--color-text) 10%, transparent);
    border-radius: 3px;
    overflow: hidden;
    min-width: 40px;
  }

  .risk-bar-fill {
    height: 100%;
    border-radius: 3px;
    transition: width 0.3s ease;
  }

  .risk-bar-low { background: #22c55e; }
  .risk-bar-medium { background: #f59e0b; }
  .risk-bar-high { background: #ef4444; }

  .risk-badge {
    font-size: 9px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: var(--radius-sm);
  }

  .risk-badge-good {
    background: color-mix(in srgb, #22c55e 15%, transparent);
    color: #bbf7d0;
    border: 1px solid color-mix(in srgb, #22c55e 30%, transparent);
  }

  .risk-badge-warn {
    background: color-mix(in srgb, #f59e0b 15%, transparent);
    color: #fde68a;
    border: 1px solid color-mix(in srgb, #f59e0b 30%, transparent);
  }

  .risk-confidence {
    font-size: 9px;
    color: var(--color-text-muted);
    font-style: italic;
  }

  .call-count-zero {
    opacity: 0.4;
  }

  
  /* ── Causal Flow Trace ─────────────────────────────────────── */
  .trace-step-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-1) 0;
  }
  .trace-step-item {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 6px var(--space-2);
    background: none;
    border: 1px solid transparent;
    border-radius: 4px;
    cursor: pointer;
    text-align: left;
    width: 100%;
    color: var(--color-text);
    font-size: var(--text-sm);
    transition: background 0.1s;
  }
  .trace-step-item:hover {
    background: var(--color-surface-elevated);
    border-color: var(--color-border);
  }
  .trace-step-item.trace-current {
    background: color-mix(in srgb, var(--color-link) 12%, transparent);
    border-color: var(--color-link);
  }
  .trace-step-num {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--color-link);
    color: white;
    font-size: 11px;
    font-weight: 700;
    flex-shrink: 0;
  }
  .trace-step-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .trace-step-name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .trace-step-type {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
  }
  .trace-step-spec {
    font-size: 10px;
    color: var(--color-success);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .trace-step-arrow {
    color: var(--color-text-muted);
    font-size: 16px;
    flex-shrink: 0;
  }

  
  /* Produced by persona */
  .detail-persona {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
  }

  .detail-persona code {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    background: color-mix(in srgb, var(--color-primary) 10%, transparent);
    padding: 1px 6px;
    border-radius: 3px;
    color: var(--color-primary);
  }

  
  /* Common flows */
  .common-flows-list {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-top: var(--space-2);
  }

  .common-flow-btn {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius);
    cursor: pointer;
    text-align: left;
    width: 100%;
    transition: border-color var(--transition-fast), background var(--transition-fast);
    color: var(--color-text);
    font-family: var(--font-body);
  }

  .common-flow-btn:hover {
    border-color: var(--color-primary);
    background: color-mix(in srgb, var(--color-primary) 8%, transparent);
  }

  .common-flow-btn:focus-visible {
    outline: 2px solid var(--color-focus);
    outline-offset: 2px;
  }

  .common-flow-icon {
    color: var(--color-primary);
    font-size: var(--text-sm);
    flex-shrink: 0;
    margin-top: 1px;
  }

  .common-flow-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .common-flow-name {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
  }

  .common-flow-desc {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    line-height: 1.4;
  }

  @media (prefers-reduced-motion: reduce) {
    .risk-bar-fill { transition: none; }
    .common-flow-btn { transition: none; }
  }

  /* Moldable View Banner */
  .moldable-view-banner {
    padding: var(--space-2) var(--space-4);
    background: rgba(15, 15, 26, 0.95);
    border-bottom: 1px solid #334155;
  }

  .moldable-view-indicator {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }

  .moldable-view-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--color-primary);
    flex-shrink: 0;
  }

  .moldable-view-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .moldable-view-label {
    font-size: var(--text-xs);
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: #e2e8f0;
    font-family: var(--font-mono);
  }

  .moldable-view-desc {
    font-size: 10px;
    color: #94a3b8;
    line-height: 1.3;
  }

  /* ── Evaluative tab styles ── */
  .eval-stats-summary {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    padding: var(--space-2) 0;
    margin-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }

  .eval-stat {
    display: flex;
    flex-direction: column;
    align-items: center;
    min-width: 50px;
    padding: var(--space-1) var(--space-2);
    background: var(--color-surface-elevated);
    border-radius: var(--radius-sm);
  }

  .eval-stat-label {
    font-size: 10px;
    color: var(--color-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .eval-stat-value {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--color-text);
    font-family: var(--font-mono);
  }

  .eval-error-rate {
    color: var(--color-error, #ef4444);
  }

  .eval-span-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .eval-span-row {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding: var(--space-2);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    text-align: left;
    transition: background 0.1s;
    font: inherit;
    color: inherit;
  }

  .eval-span-row:hover {
    background: var(--color-surface-elevated);
  }

  .eval-span-row.eval-span-error {
    border-left: 3px solid var(--color-error, #ef4444);
  }

  .eval-span-row.eval-span-expanded {
    background: var(--color-surface-elevated);
  }

  .eval-span-header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }

  .eval-span-name {
    flex: 1;
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-text);
  }

  .eval-span-duration {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--color-muted);
    white-space: nowrap;
  }

  .eval-span-status {
    font-size: 10px;
    font-weight: 700;
    padding: 1px 4px;
    border-radius: var(--radius-sm);
  }

  .eval-status-ok {
    color: var(--color-success, #22c55e);
    background: rgba(34, 197, 94, 0.1);
  }

  .eval-status-error {
    color: var(--color-error, #ef4444);
    background: rgba(239, 68, 68, 0.1);
  }

  .eval-span-detail {
    padding-top: var(--space-2);
    margin-top: var(--space-2);
    border-top: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .eval-detail-row {
    display: flex;
    gap: var(--space-2);
    font-size: var(--text-xs);
  }

  .eval-detail-label {
    color: var(--color-muted);
    min-width: 60px;
    flex-shrink: 0;
  }

  .eval-detail-value {
    color: var(--color-text);
    word-break: break-all;
  }

  .eval-detail-mono {
    font-family: var(--font-mono);
    font-size: 10px;
  }

  .eval-attributes {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .eval-attr {
    display: flex;
    gap: var(--space-1);
    font-size: 10px;
    font-family: var(--font-mono);
  }

  .eval-attr-key {
    color: var(--color-primary);
  }

  .eval-attr-val {
    color: var(--color-text);
    word-break: break-all;
  }
</style>
