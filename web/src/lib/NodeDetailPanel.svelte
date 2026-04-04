<script>
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

    // Populate usedBy: types/traits/endpoints that reference this node
    // via incoming Calls, Contains, FieldOf, or RoutesTo edges
    for (const e of edges) {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (tgt === nodeId && (et === 'calls' || et === 'field_of' || et === 'routes_to' || et === 'contains')) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode && !usedBy.some(u => u.id === srcNode.id)) usedBy.push(srcNode);
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
      case 'spec': return 'Spec';
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
    if (node.last_modified_sha && node.last_modified_sha !== node.created_sha) parts.push(`last modified in ${node.last_modified_sha.slice(0, 7)}`);
    if (modified) parts.push(`modified ${timeAgo(modified)}`);
    if (fieldCount > 0) parts.push(`${fieldCount} field${fieldCount !== 1 ? 's' : ''}`);
    if (modCount > 0) parts.push(`${modCount} change${modCount !== 1 ? 's' : ''} in last 30 days`);
    if (relatedEdges.length > 0) parts.push(`${relatedEdges.length} relationship${relatedEdges.length !== 1 ? 's' : ''}`);

    return parts.length > 0 ? parts.join('. ') + '.' : null;
  });

  // Relative time helper
  function timeAgo(date) {
    const now = new Date();
    const diffMs = now - date;
    const diffMins = Math.floor(diffMs / 60000);
    if (diffMins < 1) return 'just now';
    if (diffMins < 60) return `${diffMins}m ago`;
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    const diffDays = Math.floor(diffHours / 24);
    if (diffDays < 30) return `${diffDays}d ago`;
    return date.toLocaleDateString();
  }

  // Compute "used by" for type view: callers + incoming FieldOf edges
  let typeUsedBy = $derived.by(() => {
    if (!node || (node.node_type !== 'type' && node.node_type !== 'table' && node.node_type !== 'component')) return [];
    const usedBySet = new Map();
    // Callers
    for (const c of relationships.calledBy) {
      usedBySet.set(c.id, c);
    }
    // Incoming FieldOf: types that have a field of this type
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

  // Compute risk summary for type view
  let typeRisk = $derived.by(() => {
    if (!node || (node.node_type !== 'type' && node.node_type !== 'table' && node.node_type !== 'component')) return null;
    const churn = node.churn_count_30d ?? 0;
    const incomingCalls = relationships.calledBy.length;
    const outgoingCalls = relationships.calls.length;
    const coupling = incomingCalls + outgoingCalls;
    const couplingLabel = coupling > 20 ? 'high' : coupling > 8 ? 'medium' : 'low';
    const specCoverage = node.spec_path || relationships.governedBy ? 'high' : 'none';
    const testCoverage = node.test_coverage != null ? `${Math.round(node.test_coverage * 100)}%` : 'unknown';
    return { churn, coupling, couplingLabel, specCoverage, testCoverage };
  });

  // For endpoint view: compute call flow chain (depth 3) from handler
  let endpointFlow = $derived.by(() => {
    if (!node || node.node_type !== 'endpoint') return [];
    // Find handler(s) via RoutesTo
    const handlerIds = new Set();
    for (const e of edges) {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et === 'routes_to' && src === node.id) {
        handlerIds.add(e.target_id ?? e.to_node_id ?? e.to);
      }
    }
    if (handlerIds.size === 0) return [];

    // BFS from handlers, depth 3, following Calls edges
    const visited = new Set();
    const chain = []; // array of { node, depth }
    let frontier = [...handlerIds];
    // Include handlers themselves as Step 0 in the flow chain
    for (const hid of frontier) {
      visited.add(hid);
      const handlerNode = nodes.find(n => n.id === hid);
      if (handlerNode) chain.push({ node: handlerNode, depth: 0 });
    }

    for (let depth = 1; depth <= 3; depth++) {
      const nextFrontier = [];
      for (const currentId of frontier) {
        for (const e of edges) {
          const src = e.source_id ?? e.from_node_id ?? e.from;
          const tgt = e.target_id ?? e.to_node_id ?? e.to;
          const et = (e.edge_type ?? e.type ?? '').toLowerCase();
          if (et === 'calls' && src === currentId && !visited.has(tgt)) {
            visited.add(tgt);
            const targetNode = nodes.find(n => n.id === tgt);
            if (targetNode) {
              chain.push({ node: targetNode, depth });
              nextFrontier.push(tgt);
            }
          }
        }
      }
      frontier = nextFrontier;
    }
    return chain;
  });

  // For endpoint view: count test functions that can reach this endpoint transitively
  let endpointTestCount = $derived.by(() => {
    if (!node || node.node_type !== 'endpoint') return 0;
    // Direct tests
    let count = relationships.testedBy.length;
    if (count > 0) return count;
    // Check if any test node can reach the handler(s) via Calls/RoutesTo
    const handlerIds = new Set();
    for (const e of edges) {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et === 'routes_to' && src === node.id) {
        handlerIds.add(e.target_id ?? e.to_node_id ?? e.to);
      }
    }
    // Find test nodes that call handlers directly
    const testNodes = nodes.filter(n => n.test_node);
    for (const tn of testNodes) {
      for (const e of edges) {
        const src = e.source_id ?? e.from_node_id ?? e.from;
        const tgt = e.target_id ?? e.to_node_id ?? e.to;
        const et = (e.edge_type ?? e.type ?? '').toLowerCase();
        if ((et === 'calls' || et === 'routes_to') && src === tn.id && (handlerIds.has(tgt) || tgt === node.id)) {
          count++;
          break;
        }
      }
    }
    return count;
  });

  // For endpoint view: extract gate metadata
  let endpointGates = $derived.by(() => {
    if (!node || node.node_type !== 'endpoint') return [];
    const gates = [];
    // Check metadata fields
    if (node.metadata) {
      try {
        const meta = typeof node.metadata === 'string' ? JSON.parse(node.metadata) : node.metadata;
        if (meta.gates) gates.push(...(Array.isArray(meta.gates) ? meta.gates : [meta.gates]));
        if (meta.gate) gates.push(meta.gate);
        if (meta.role_required) gates.push(`Role: ${meta.role_required}`);
        if (meta.auth_required !== undefined) gates.push(meta.auth_required ? 'Auth required' : 'Public');
      } catch (e) {
        gates.push(`[malformed gate metadata: ${e.message}]`);
      }
    }
    // Check doc_comment for gate hints
    if (node.doc_comment) {
      const gateMatch = node.doc_comment.match(/\[(.*?)\]/g);
      if (gateMatch) {
        for (const g of gateMatch) {
          const inner = g.slice(1, -1);
          if (/admin|auth|role|gate|guard|require/i.test(inner)) gates.push(inner);
        }
      }
    }
    return gates;
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

  // For spec view: find all nodes governed by this spec
  let specGovernedNodes = $derived.by(() => {
    if (!node || node.node_type !== 'spec') return [];
    const specId = node.id;
    const specPath = node.spec_path ?? node.file_path ?? node.name;
    const governed = new Map();

    // Nodes with matching spec_path
    for (const n of nodes) {
      if (n.id === specId) continue;
      if (n.spec_path && (n.spec_path === specPath || n.spec_path === node.name || n.spec_path === node.qualified_name)) {
        governed.set(n.id, n);
      }
    }

    // Nodes with GovernedBy edges pointing to this spec
    for (const e of edges) {
      const src = e.source_id ?? e.from_node_id ?? e.from;
      const tgt = e.target_id ?? e.to_node_id ?? e.to;
      const et = (e.edge_type ?? e.type ?? '').toLowerCase();
      if (et === 'governed_by' && tgt === specId) {
        const srcNode = nodes.find(n => n.id === src);
        if (srcNode) governed.set(srcNode.id, srcNode);
      }
    }

    return [...governed.values()];
  });

  // For spec view: compute implementation completeness
  let specCompleteness = $derived.by(() => {
    if (!node || node.node_type !== 'spec') return null;
    const total = specGovernedNodes.length;
    if (total === 0) return { total: 0, tested: 0, pct: 0 };

    let tested = 0;
    for (const gn of specGovernedNodes) {
      if (gn.test_coverage != null && gn.test_coverage > 0) {
        tested++;
      } else if (gn.test_node) {
        tested++;
      } else {
        // Check if any test node calls this governed node
        for (const e of edges) {
          const src = e.source_id ?? e.from_node_id ?? e.from;
          const tgt = e.target_id ?? e.to_node_id ?? e.to;
          const et = (e.edge_type ?? e.type ?? '').toLowerCase();
          if ((et === 'tests' || et === 'calls') && tgt === gn.id) {
            const srcNode = nodes.find(n => n.id === src);
            if (srcNode?.test_node) { tested++; break; }
          }
        }
      }
    }

    const pct = total > 0 ? Math.round((tested / total) * 100) : 0;
    return { total, tested, pct };
  });

  // For spec view: group governed nodes by type
  let specNodesByType = $derived.by(() => {
    if (!node || node.node_type !== 'spec') return new Map();
    const groups = new Map();
    for (const gn of specGovernedNodes) {
      const t = gn.node_type ?? 'unknown';
      if (!groups.has(t)) groups.set(t, []);
      groups.get(t).push(gn);
    }
    return groups;
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
        <!-- Spec link (clickable) -->
        {#if node.spec_path || relationships.governedBy}
          <div class="detail-section">
            <h4 class="detail-section-title">Spec</h4>
            <button class="detail-spec-button" onclick={() => onNavigate({ id: node.spec_path ?? relationships.governedBy, name: node.spec_path ?? relationships.governedBy, node_type: 'spec' })} type="button">
              {node.spec_path ?? relationships.governedBy}
            </button>
          </div>
        {/if}

        <!-- Last modified summary -->
        {#if node.last_modified_by || node.last_modified_at}
          <div class="detail-section">
            <p class="detail-modified-summary">
              {#if node.last_modified_by}Last modified by <code>{node.last_modified_by}</code>{/if}{#if node.last_modified_at}{node.last_modified_by ? ', ' : 'Last modified '}{timeAgo(new Date(node.last_modified_at * 1000))}{/if}
            </p>
          </div>
        {/if}

        <!-- Fields -->
        {#if relationships.fields.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Fields ({relationships.fields.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.fields as f}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(f)} type="button">
                    <span class="ref-type">{f.node_type}</span>
                    <span class="field-name">{f.name}</span>
                    {#if f.qualified_name && f.qualified_name !== f.name}
                      <span class="field-type-annotation">: {f.qualified_name.split('::').pop()}</span>
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

        <!-- Used By: callers + incoming FieldOf edges -->
        {#if typeUsedBy.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Used By ({typeUsedBy.length})</summary>
            <ul class="detail-ref-list">
              {#each typeUsedBy.slice(0, 15) as user}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(user)} type="button">
                    <span class="ref-type">{user.node_type}</span> {user.name}
                  </button>
                </li>
              {/each}
              {#if typeUsedBy.length > 15}
                <li class="detail-more">+{typeUsedBy.length - 15} more</li>
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
            {#if node.created_sha}
              <p class="detail-provenance">Created in <code>{node.created_sha.slice(0, 7)}</code></p>
            {/if}
            {#if node.last_modified_sha}
              <p class="detail-provenance">Last modified in <code>{node.last_modified_sha.slice(0, 7)}</code>{#if node.last_modified_at}, {timeAgo(new Date(node.last_modified_at * 1000))}{/if}</p>
            {/if}
            {#if node.churn_count_30d}
              <p class="detail-provenance">{node.churn_count_30d} modification{node.churn_count_30d !== 1 ? 's' : ''} in last 30 days</p>
            {/if}
          </div>
          {#if !story && !node.doc_comment && !node.created_sha}
            <p class="detail-story detail-muted">No documentation or history available.</p>
          {/if}
        </details>

        <!-- Risk Summary (type-specific) -->
        {#if typeRisk}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Risk Summary</summary>
            <div class="risk-summary-grid">
              <span class="risk-metric">
                <span class="risk-metric-label">Churn</span>
                <span class="risk-metric-value">{typeRisk.churn}/30d</span>
              </span>
              <span class="risk-metric">
                <span class="risk-metric-label">Coupling</span>
                <span class="risk-metric-value risk-coupling-{typeRisk.couplingLabel}">{typeRisk.couplingLabel} ({typeRisk.coupling})</span>
              </span>
              <span class="risk-metric">
                <span class="risk-metric-label">Spec</span>
                <span class="risk-metric-value">{typeRisk.specCoverage}</span>
              </span>
              <span class="risk-metric">
                <span class="risk-metric-label">Tests</span>
                <span class="risk-metric-value">{typeRisk.testCoverage}</span>
              </span>
            </div>
          </details>
        {/if}

      <!-- ============================================ -->
      <!-- TRAIT / INTERFACE VIEW: methods / implementations / dependents -->
      <!-- ============================================ -->
      {:else if node.node_type === 'interface' || node.node_type === 'trait'}
        <!-- Spec link -->
        {#if node.spec_path || relationships.governedBy}
          <div class="detail-section">
            <h4 class="detail-section-title">Spec</h4>
            <button class="detail-spec-button" onclick={() => onNavigate({ id: node.spec_path ?? relationships.governedBy, name: node.spec_path ?? relationships.governedBy, node_type: 'spec' })} type="button">
              {node.spec_path ?? relationships.governedBy}
            </button>
          </div>
        {/if}

        <!-- Crate / module info -->
        {#if node.qualified_name}
          {@const crateName = node.qualified_name.split('::')[0]}
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
            <summary class="detail-section-title">Implementations ({relationships.implementedBy.length})</summary>
            <ul class="detail-ref-list">
              {#each relationships.implementedBy as impl}
                <li>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(impl)} type="button">
                    <span class="ref-type">{impl.node_type}</span> {impl.name}
                    {#if impl.file_path}
                      <span class="impl-location">{impl.file_path.split('/').pop()}</span>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <!-- Dependents (who calls methods on this trait) -->
        {#if relationships.calledBy.length > 0}
          <details class="detail-collapsible" open>
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

      <!-- ============================================ -->
      <!-- ENDPOINT VIEW: route / handler / flow / gates / tests -->
      <!-- ============================================ -->
      {:else if node.node_type === 'endpoint'}
        <!-- Route info (method + path) -->
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

        <!-- Spec link -->
        {#if node.spec_path || relationships.governedBy}
          <div class="detail-section">
            <h4 class="detail-section-title">Spec</h4>
            <button class="detail-spec-button" onclick={() => onNavigate({ id: node.spec_path ?? relationships.governedBy, name: node.spec_path ?? relationships.governedBy, node_type: 'spec' })} type="button">
              {node.spec_path ?? relationships.governedBy}
            </button>
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
                    {#if handler.file_path}
                      <span class="handler-location">{handler.file_path.split('/').pop()}{handler.line_start ? `:${handler.line_start}` : ''}</span>
                    {/if}
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
                    {#if handler.file_path}
                      <span class="handler-location">{handler.file_path.split('/').pop()}{handler.line_start ? `:${handler.line_start}` : ''}</span>
                    {/if}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <!-- Flow: call chain from handler, depth 3 -->
        {#if endpointFlow.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Flow ({endpointFlow.length})</summary>
            <ul class="detail-ref-list flow-chain">
              {#each endpointFlow as step}
                <li class="flow-step" style="padding-left: {step.depth * 12}px">
                  <span class="flow-arrow">{step.depth === 1 ? '' : ''}</span>
                  <button class="detail-ref-link" onclick={() => handleNodeClick(step.node)} type="button">
                    <span class="ref-type">{step.node.node_type}</span> {step.node.name}
                  </button>
                </li>
              {/each}
            </ul>
          </details>
        {/if}

        <!-- Gates -->
        {#if endpointGates.length > 0}
          <div class="detail-section">
            <h4 class="detail-section-title">Gates</h4>
            <div class="gate-list">
              {#each endpointGates as gate}
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
                {#each relationships.testedBy as testNode}
                  <li>
                    <button class="detail-ref-link" onclick={() => handleNodeClick(testNode)} type="button">
                      <span class="ref-type">test</span> {testNode.name}
                    </button>
                  </li>
                {/each}
              </ul>
            </details>
          {:else if endpointTestCount > 0}
            <p class="detail-test-count">{endpointTestCount} test{endpointTestCount !== 1 ? 's' : ''} reach this endpoint</p>
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

      <!-- ============================================ -->
      <!-- SPEC VIEW: content preview / completeness / linked nodes -->
      <!-- ============================================ -->
      {:else if node.node_type === 'spec'}
        <!-- Spec content preview -->
        {#if node.doc_comment || node.description}
          <div class="detail-section">
            <h4 class="detail-section-title">Content Preview</h4>
            <div class="spec-content-preview">
              <p class="detail-doc">{node.doc_comment ?? node.description}</p>
            </div>
          </div>
        {/if}

        <!-- File path for spec -->
        {#if node.spec_path && node.spec_path !== node.file_path}
          <div class="detail-section">
            <h4 class="detail-section-title">Spec Path</h4>
            <p class="detail-file"><code>{node.spec_path}</code></p>
          </div>
        {/if}

        <!-- Implementation Completeness -->
        {#if specCompleteness}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Implementation Completeness</summary>
            <div class="spec-completeness-meter">
              <div class="completeness-bar">
                <div class="completeness-fill" style="width: {specCompleteness.pct}%"></div>
                <div class="completeness-tested-fill" style="width: {specCompleteness.pct}%"></div>
              </div>
              <div class="completeness-stats">
                <span class="completeness-stat">
                  <span class="completeness-stat-value">{specCompleteness.total}</span>
                  <span class="completeness-stat-label">linked node{specCompleteness.total !== 1 ? 's' : ''}</span>
                </span>
                <span class="completeness-stat">
                  <span class="completeness-stat-value">{specCompleteness.tested}</span>
                  <span class="completeness-stat-label">tested</span>
                </span>
                <span class="completeness-stat">
                  <span class="completeness-stat-value">{specCompleteness.pct}%</span>
                  <span class="completeness-stat-label">test coverage</span>
                </span>
              </div>
            </div>
            {#if specCompleteness.total === 0}
              <p class="detail-story detail-muted">No implementation nodes linked to this spec.</p>
            {/if}
          </details>
        {/if}

        <!-- Linked nodes by type -->
        {#if specGovernedNodes.length > 0}
          <details class="detail-collapsible" open>
            <summary class="detail-section-title">Linked Nodes ({specGovernedNodes.length})</summary>
            {#each [...specNodesByType.entries()] as [nodeType, typeNodes]}
              <div class="spec-type-group">
                <h5 class="spec-type-group-label">{nodeType} ({typeNodes.length})</h5>
                <ul class="detail-ref-list">
                  {#each typeNodes.slice(0, 10) as gn}
                    <li>
                      <button class="detail-ref-link" onclick={() => handleNodeClick(gn)} type="button">
                        <span class="ref-type">{gn.node_type}</span>
                        <span class="field-name">{gn.name}</span>
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

        <!-- Story -->
        {#if story}
          <div class="detail-section">
            <h4 class="detail-section-title">Story</h4>
            <p class="detail-story">{story}</p>
          </div>
        {/if}

      <!-- ============================================ -->
      <!-- EDGE VIEW: relationship between two nodes -->
      <!-- ============================================ -->
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
            <code class:span-error={node.status === 'error' || node.status === 'ERROR'}>{node.status}</code>
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

  /* Modified summary in type header */
  .detail-modified-summary {
    font-size: var(--text-xs);
    color: var(--color-text-muted);
    margin: 0;
  }

  .detail-modified-summary code {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: color-mix(in srgb, var(--color-text) 8%, transparent);
    padding: 1px 4px;
    border-radius: 3px;
  }

  /* Field type annotations */
  .field-name {
    color: var(--color-link);
  }

  .field-type-annotation {
    color: var(--color-text-muted);
    font-size: var(--text-xs);
    margin-left: 2px;
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

  /* Crate info for trait view */
  .detail-crate {
    font-size: var(--text-sm);
    color: var(--color-text-secondary);
    margin: 0;
  }

  .detail-crate code {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    background: color-mix(in srgb, var(--color-text) 8%, transparent);
    padding: 1px 6px;
    border-radius: 3px;
  }

  /* Implementation location hint */
  .impl-location, .handler-location {
    margin-left: auto;
    font-size: 9px;
    color: var(--color-text-muted);
    flex-shrink: 0;
  }

  /* HTTP method badges */
  .http-method {
    font-size: var(--text-xs);
    font-weight: 700;
    font-family: var(--font-mono);
    padding: 1px 6px;
    border-radius: 3px;
    margin-right: var(--space-1);
  }

  .http-get { background: color-mix(in srgb, #22c55e 20%, transparent); color: #bbf7d0; }
  .http-post { background: color-mix(in srgb, #3b82f6 20%, transparent); color: #bfdbfe; }
  .http-put { background: color-mix(in srgb, #f59e0b 20%, transparent); color: #fde68a; }
  .http-patch { background: color-mix(in srgb, #f59e0b 20%, transparent); color: #fde68a; }
  .http-delete { background: color-mix(in srgb, #ef4444 20%, transparent); color: #fca5a5; }

  /* Flow chain */
  .flow-chain {
    gap: 0;
  }

  .flow-step {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .flow-arrow {
    font-size: 10px;
    color: var(--color-text-muted);
    flex-shrink: 0;
    width: 14px;
  }

  .flow-step .detail-ref-link {
    flex: 1;
  }

  /* Gate badges */
  .gate-list {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
  }

  .gate-badge {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    padding: 2px 8px;
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    color: var(--color-primary);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
  }

  /* Test count text */
  .detail-test-count {
    font-size: var(--text-xs);
    color: var(--color-text-secondary);
    margin: 0;
  }

  /* Spec view styles */
  .spec-content-preview {
    max-height: 120px;
    overflow-y: auto;
    padding: var(--space-2);
    background: var(--color-surface-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
  }

  .spec-completeness-meter {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .completeness-bar {
    position: relative;
    width: 100%;
    height: 8px;
    background: color-mix(in srgb, var(--color-text) 10%, transparent);
    border-radius: 4px;
    overflow: hidden;
  }

  .completeness-fill {
    position: absolute;
    top: 0;
    left: 0;
    height: 100%;
    background: color-mix(in srgb, var(--color-primary) 30%, transparent);
    border-radius: 4px;
    transition: width 0.3s ease;
  }

  .completeness-tested-fill {
    position: absolute;
    top: 0;
    left: 0;
    height: 100%;
    background: #22c55e;
    border-radius: 4px;
    transition: width 0.3s ease;
  }

  .completeness-stats {
    display: flex;
    gap: var(--space-3);
  }

  .completeness-stat {
    display: flex;
    flex-direction: column;
    align-items: center;
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

  .spec-type-group {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: var(--space-2);
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

  .spec-coverage-badge {
    margin-left: auto;
    font-size: 9px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: var(--radius-sm);
    flex-shrink: 0;
  }

  .spec-coverage-badge.coverage-good {
    background: color-mix(in srgb, #22c55e 15%, transparent);
    color: #bbf7d0;
    border: 1px solid color-mix(in srgb, #22c55e 30%, transparent);
  }

  .spec-coverage-badge.coverage-poor {
    background: color-mix(in srgb, #ef4444 15%, transparent);
    color: #fca5a5;
    border: 1px solid color-mix(in srgb, #ef4444 30%, transparent);
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
</style>
