<script>
  import { t } from 'svelte-i18n';

  let {
    node = null,
    nodes = [],
    edges = [],
    onClose = () => {},
    onNavigate = () => {},
  } = $props();

  // Compute relationships for the selected node
  let relationships = $derived.by(() => {
    if (!node) return { implementedBy: [], implements: [], calledBy: [], calls: [], fields: [], containedIn: null, contains: [], governedBy: null, usedBy: [], routesTo: [], testedBy: [], methods: [] };
    const nodeId = node.id;

    const implementedBy = [];
    const implementsTraits = [];
    const calledBy = [];
    const callsOut = [];
    const fields = [];
    const contains = [];
    const usedBy = [];
    const routesTo = [];
    const testedBy = [];
    const methods = [];
    let containedIn = null;
    let governedBy = null;

    for (const e of edges) {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();

      if (et === 'implements' && tgt === nodeId) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) implementedBy.push(srcNode);
      }
      if (et === 'implements' && src === nodeId) {
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode) implementsTraits.push(tgtNode);
      }
      if (et === 'calls' && tgt === nodeId) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) calledBy.push(srcNode);
      }
      if (et === 'calls' && src === nodeId) {
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode) callsOut.push(tgtNode);
      }
      if (et === 'field_of' && src === nodeId) {
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode) fields.push(tgtNode);
      }
      if (et === 'field_of' && tgt === nodeId) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) fields.push(srcNode);
      }
      if (et === 'contains' && src === nodeId) {
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode) {
          contains.push(tgtNode);
          // Methods are functions contained in a trait/interface
          if (tgtNode.node_type === 'function') methods.push(tgtNode);
        }
      }
      if (et === 'contains' && tgt === nodeId) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) containedIn = srcNode;
      }
      if (et === 'governed_by' && src === nodeId) {
        governedBy = tgt; // spec path or node id
      }
      if (et === 'routes_to' && src === nodeId) {
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode) routesTo.push(tgtNode);
      }
      if (et === 'tests' && tgt === nodeId) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) testedBy.push(srcNode);
      }
      if (et === 'tests' && src === nodeId) {
        const tgtNode = nodes.find(n => n.id === tgt);
        if (tgtNode) testedBy.push(tgtNode);
      }
    }

    // Also find test nodes that call this node (tests often just call the function under test)
    if (testedBy.length === 0) {
      for (const caller of calledBy) {
        if (caller.test_node) testedBy.push(caller);
      }
    }

    return { implementedBy, implements: implementsTraits, calledBy, calls: callsOut, fields, containedIn, contains, governedBy, usedBy, routesTo, testedBy, methods };
  });

  // Compute call-site counts for trait methods (how many call edges target each method)
  let methodCallCounts = $derived.by(() => {
    if (!node || (node.node_type !== 'interface' && node.node_type !== 'trait')) return new Map();
    const counts = new Map();
    for (const method of relationships.methods) {
      let count = 0;
      for (const e of edges) {
        const tgt = e.target_id ?? e.to_node_id ?? e.to;
        const et = (e.edge_type ?? e.type ?? '').toLowerCase();
        if (et === 'calls' && tgt === method.id) count++;
      }
      counts.set(method.id, count);
    }
    return counts;
  });

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
      default: return node.node_type ?? 'Unknown';
    }
  });

  let visibilityBadge = $derived.by(() => {
    if (!node) return '';
    const v = (node.visibility ?? '').toLowerCase();
    return v === 'public' ? 'pub' : v === 'private' ? 'priv' : v;
  });

  // Compute story: how the node evolved over milestones/commits
  let story = $derived.by(() => {
    if (!node) return null;
    const created = node.created_at ? new Date(node.created_at * 1000) : null;
    const modified = node.last_modified_at ? new Date(node.last_modified_at * 1000) : null;
    const firstSeen = node.first_seen_at ? new Date(node.first_seen_at * 1000) : null;

    // Count modifications (field changes, related edges added)
    const relatedEdges = edges.filter(e => {
      const src = e.source_id ?? e.from;
      const tgt = e.target_id ?? e.to;
      return src === node.id || tgt === node.id;
    });

    const modCount = node.churn_count_30d ?? 0;
    const fieldCount = relationships.fields.length;

    let parts = [];
    if (created) parts.push(`Created ${created.toLocaleDateString()}`);
    if (node.created_sha) parts.push(`in commit ${node.created_sha.slice(0, 7)}`);
    if (fieldCount > 0) parts.push(`${fieldCount} field${fieldCount !== 1 ? 's' : ''}`);
    if (modCount > 0) parts.push(`${modCount} change${modCount !== 1 ? 's' : ''} in last 30 days`);
    if (relatedEdges.length > 0) parts.push(`${relatedEdges.length} relationship${relatedEdges.length !== 1 ? 's' : ''}`);

    return parts.length > 0 ? parts.join('. ') + '.' : null;
  });

  // For endpoint nodes: extract request/response info from metadata
  let endpointMeta = $derived.by(() => {
    if (!node || node.node_type !== 'endpoint') return null;
    // Look for RoutesTo edges from this endpoint
    const routesTo = [];
    for (const e of edges) {
      const src = e.source_id ?? e.from;
      const et = (e.edge_type ?? '').toLowerCase();
      if (et === 'routes_to' && src === node.id) {
        const handler = nodes.find(n => n.id === (e.target_id ?? e.to));
        if (handler) routesTo.push(handler);
      }
    }
    // Parse metadata if available
    let method = '';
    let path = '';
    try {
      if (node.doc_comment) {
        const parts = node.doc_comment.match(/^(GET|POST|PUT|DELETE|PATCH)\s+(.+)/);
        if (parts) { method = parts[1]; path = parts[2]; }
      }
    } catch(e) {}
    return { routesTo, method, path };
  });

  function handleNodeClick(targetNode) {
    if (targetNode) {
      onNavigate(targetNode);
    }
  }
</script>

{#if node}
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

      <!-- ============================================ -->
      <!-- TYPE VIEW: fields / implements / used-by / story / risk -->
      <!-- ============================================ -->
      {#if node.node_type === 'type' || node.node_type === 'table' || node.node_type === 'component'}
        <!-- Fields -->
        {#if relationships.fields.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Fields ({relationships.fields.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.fields as f}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(f)} type="button">
                    <span class="ref-type">{f.node_type}</span> {f.name}
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

        <!-- Used By / Dependents -->
        {#if relationships.calledBy.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Used By ({relationships.calledBy.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.calledBy.slice(0, 15) as caller}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(caller)} type="button">
                    <span class="ref-type">{caller.node_type}</span> {caller.name}
                  </button>
                </li>
              {/each}
              {#if relationships.calledBy.length > 15}
                <li class="detail-more">+{relationships.calledBy.length - 15} more</li>
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
          {#if node.spec_path || relationships.governedBy}
            <p class="detail-spec-link">Spec: <span class="detail-spec">{node.spec_path ?? relationships.governedBy}</span></p>
          {/if}
          {#if story}
            <p class="detail-story">{story}</p>
          {:else if !node.doc_comment && !node.spec_path && !relationships.governedBy}
            <p class="detail-story detail-muted">No documentation or history available.</p>
          {/if}
        </details>

      <!-- ============================================ -->
      <!-- TRAIT / INTERFACE VIEW: methods / implementations / dependents -->
      <!-- ============================================ -->
      {:else if node.node_type === 'interface' || node.node_type === 'trait'}
        <!-- Methods (contained functions) -->
        {#if relationships.methods.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Methods ({relationships.methods.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.methods as method}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(method)} type="button">
                    <span class="ref-type">fn</span> {method.name}
                    {#if methodCallCounts.get(method.id)}
                      <span class="call-count" title="Call sites">{methodCallCounts.get(method.id)} call{methodCallCounts.get(method.id) !== 1 ? 's' : ''}</span>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <!-- Implementors -->
        {#if relationships.implementedBy.length > 0}
          <details class="detail-collapsible" open>
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

        <!-- Dependents (who calls methods on this trait) -->
        {#if relationships.calledBy.length > 0}
          <details class="detail-collapsible">
            <summary class="detail-section-title">Dependents ({relationships.calledBy.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.calledBy.slice(0, 15) as caller}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(caller)} type="button">
                    <span class="ref-type">{caller.node_type}</span> {caller.name}
                  </button>
                </li>
              {/each}
              {#if relationships.calledBy.length > 15}
                <li class="detail-more">+{relationships.calledBy.length - 15} more</li>
              {/if}
            </ul>
          </details>
        {/if}

        <!-- Doc comment -->
        {#if node.doc_comment}
          <div class="detail-section">
            <h4 class="detail-section-title">Documentation</h4>
            <p class="detail-doc">{node.doc_comment}</p>
          </div>
        {/if}

      <!-- ============================================ -->
      <!-- ENDPOINT VIEW: route / handler / request-response / tests -->
      <!-- ============================================ -->
      {:else if node.node_type === 'endpoint'}
        <!-- Route info -->
        {#if endpointMeta}
          <div class="detail-section">
            <h4 class="detail-section-title">Route</h4>
            {#if endpointMeta.method || endpointMeta.path}
              <p class="detail-endpoint-method"><code>{endpointMeta.method} {endpointMeta.path}</code></p>
            {/if}
          </div>
        {/if}

        <!-- Handler -->
        {#if relationships.routesTo.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Handler</summary>
            <ul class="detail-ref-list">
              {#each relationships.routesTo as handler}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(handler)} type="button">
                    <span class="ref-type">{handler.node_type}</span> {handler.name}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {:else if endpointMeta && endpointMeta.routesTo.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Handler</summary>
            <ul class="detail-ref-list">
              {#each endpointMeta.routesTo as handler}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(handler)} type="button">
                    <span class="ref-type">{handler.node_type}</span> {handler.name}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <!-- Request/Response types (types this endpoint calls or references) -->
        {#if relationships.calls.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Request/Response Flow ({relationships.calls.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.calls as callee}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(callee)} type="button">
                    <span class="ref-type">{callee.node_type}</span> {callee.name}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <!-- Spec / Gates -->
        {#if node.spec_path || relationships.governedBy}
          <div class="detail-section">
            <h4 class="detail-section-title">Gates / Spec</h4>
            <p class="detail-spec">{node.spec_path ?? relationships.governedBy}</p>
          </div>
        {/if}

        <!-- Connected tests -->
        {#if relationships.testedBy.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Tests ({relationships.testedBy.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.testedBy as testNode}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(testNode)} type="button">
                    <span class="ref-type">test</span> {testNode.name}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <!-- Doc comment -->
        {#if node.doc_comment}
          <div class="detail-section">
            <h4 class="detail-section-title">Documentation</h4>
            <p class="detail-doc">{node.doc_comment}</p>
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
            <p class="detail-spec">{node.spec_path ?? relationships.governedBy}</p>
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
        {#if story}
          <div class="detail-section">
            <h4 class="detail-section-title">Story</h4>
            <p class="detail-story">{story}</p>
          </div>
        {/if}
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

  .call-count {
    margin-left: auto;
    font-size: 9px;
    font-weight: 600;
    color: var(--color-text-muted);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    padding: 1px 5px;
    flex-shrink: 0;
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

  .detail-endpoint-method code {
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    font-weight: 600;
    color: var(--color-primary);
  }

  @media (prefers-reduced-motion: reduce) {
    .detail-ref-link { transition: none; }
    .detail-collapsible > summary::before { transition: none; }
  }
</style>
