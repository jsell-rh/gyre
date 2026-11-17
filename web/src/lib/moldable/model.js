/**
 * moldable/model.js — pure computation layer for the Moldable Views
 * (system-explorer.md §2).
 *
 * Every function here derives view data from the knowledge graph
 * (`GET /repos/:id/graph` nodes + edges, see gyre-common/src/graph.rs for
 * the Rust-side data model). Extracted from NodeDetailPanel.svelte so the
 * four Moldable*View components and their tests share ONE implementation
 * (no mirrored logic in tests — they import these functions directly).
 */

/** Map a graph node_type to its moldable view kind.
 *
 * type/table/component → 'type', interface/trait → 'trait',
 * endpoint → 'endpoint', spec → 'spec', anything else → null (generic view).
 */
export function moldableViewType(nodeType) {
  switch (nodeType) {
    case 'type':
    case 'table':
    case 'component':
      return 'type';
    case 'interface':
    case 'trait':
      return 'trait';
    case 'endpoint':
      return 'endpoint';
    case 'spec':
      return 'spec';
    default:
      return null;
  }
}

/** Relative-time formatter ("2d ago") shared by every view's Story section. */
export function timeAgo(date) {
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

/** Short visibility badge: public → "pub", private → "priv". */
export function visibilityBadge(visibility) {
  const v = (visibility ?? '').toLowerCase();
  return v === 'public' ? 'pub' : v === 'private' ? 'priv' : v;
}

const edgeSource = (e) => e.source_id ?? e.from_node_id ?? e.from;
const edgeTarget = (e) => e.target_id ?? e.to_node_id ?? e.to;
const edgeType = (e) => (e.edge_type ?? e.type ?? '').toLowerCase();

/** Compute all relationships for `node` from the edge list.
 *
 * Returns { implementedBy, implements, calledBy, calls, fields, containedIn,
 * contains, governedBy, usedBy, routesTo, testedBy, methods }.
 */
export function computeRelationships(node, nodes, edges) {
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
    const src = edgeSource(e);
    const tgt = edgeTarget(e);
    const et = edgeType(e);

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

  // Also find test nodes that call this node (tests often just call the
  // function under test)
  if (testedBy.length === 0) {
    for (const caller of calledBy) {
      if (caller.test_node) testedBy.push(caller);
    }
  }

  // Populate usedBy: types/traits/endpoints that reference this node
  // via incoming Calls, Contains, FieldOf, or RoutesTo edges
  for (const e of edges) {
    const src = edgeSource(e);
    const tgt = edgeTarget(e);
    const et = edgeType(e);
    if (tgt === nodeId && (et === 'calls' || et === 'field_of' || et === 'routes_to' || et === 'contains')) {
      const srcNode = nodes.find(n => n.id === src);
      if (srcNode && !usedBy.some(u => u.id === srcNode.id)) usedBy.push(srcNode);
    }
  }

  return { implementedBy, implements: implementsTraits, calledBy, calls: callsOut, fields, containedIn, contains, governedBy, usedBy, routesTo, testedBy, methods };
}

/** Call-site counts for a trait's methods (how many call edges target each method). */
export function computeMethodCallCounts(node, relationships, edges) {
  if (!node || (node.node_type !== 'interface' && node.node_type !== 'trait')) return new Map();
  const counts = new Map();
  for (const method of relationships.methods) {
    let count = 0;
    for (const e of edges) {
      const et = edgeType(e);
      if (et === 'calls' && edgeTarget(e) === method.id) count++;
    }
    counts.set(method.id, count);
  }
  return counts;
}

/** Call-site counts for each DEPENDENT of a trait (system-explorer.md §2
 * Trait View: "Dependents list (who uses this trait, with call site counts)").
 *
 * A dependent's count is the number of call edges from it to any of the
 * trait's methods (dynamic dispatch through the trait object) plus direct
 * call edges to the trait node itself.
 */
export function computeDependentCallCounts(node, relationships, edges) {
  if (!node) return new Map();
  const methodIds = new Set(relationships.methods.map(m => m.id));
  const counts = new Map();
  for (const dependent of relationships.calledBy) {
    let count = 0;
    for (const e of edges) {
      if (edgeType(e) !== 'calls') continue;
      if (edgeSource(e) !== dependent.id) continue;
      const tgt = edgeTarget(e);
      if (methodIds.has(tgt) || tgt === node.id) count++;
    }
    counts.set(dependent.id, count);
  }
  return counts;
}

/** The dependents of a trait: every node that calls the trait itself OR any
 * of its methods, each with its call-site count into this trait. Sorted by
 * call count (heaviest user first). Callers that only target the trait node
 * directly are included too.
 */
export function computeTraitDependents(node, relationships, edges, nodes) {
  if (!node) return [];
  const methodIds = new Set(relationships.methods.map(m => m.id));
  const dependents = new Map();
  const addCall = (srcId) => {
    let d = dependents.get(srcId);
    if (!d) {
      const srcNode = nodes.find(n => n.id === srcId);
      if (!srcNode) return;
      d = { node: srcNode, calls: 0 };
      dependents.set(srcId, d);
    }
    d.calls++;
  };
  for (const e of edges) {
    if (edgeType(e) !== 'calls') continue;
    const tgt = edgeTarget(e);
    if (tgt === node.id || methodIds.has(tgt)) addCall(edgeSource(e));
  }
  return [...dependents.values()].sort((a, b) => b.calls - a.calls);
}

/** Risk metrics for the risk footer (churn / coupling / spec / tests). */
export function computeRisk(node, relationships) {
  if (!node) return null;
  const applicableTypes = ['type', 'table', 'component', 'function', 'endpoint', 'interface', 'trait'];
  if (!applicableTypes.includes(node.node_type)) return null;
  const churn = node.churn_count_30d ?? 0;
  const maxChurn = 30; // normalize churn bar against 30 changes/month
  const churnPct = Math.min(100, Math.round((churn / maxChurn) * 100));
  const churnLabel = churn > 15 ? 'high' : churn > 5 ? 'medium' : 'low';
  const incomingCalls = relationships.calledBy.length;
  const outgoingCalls = relationships.calls.length;
  const coupling = incomingCalls + outgoingCalls;
  const couplingLabel = coupling > 20 ? 'high' : coupling > 8 ? 'medium' : 'low';
  const hasSpec = !!(node.spec_path || relationships.governedBy);
  const specCoverage = hasSpec ? 'covered' : 'none';
  const specConfidence = hasSpec ? (node.spec_confidence ?? 'high') : 'none';
  const testCoverage = node.test_coverage != null ? Math.round(node.test_coverage * 100) : null;
  const testReachable = relationships.testedBy.length > 0;
  const testLabel = testCoverage != null ? `${testCoverage}%` : testReachable ? 'reachable' : 'unknown';
  const complexity = node.complexity ?? null;
  const maxComplexity = 50;
  const complexityPct = complexity != null ? Math.min(100, Math.round((complexity / maxComplexity) * 100)) : null;
  const complexityLabel = complexity != null ? (complexity > 30 ? 'high' : complexity > 15 ? 'medium' : 'low') : null;
  return { churn, churnPct, churnLabel, coupling, couplingLabel, hasSpec, specCoverage, specConfidence, testCoverage, testReachable, testLabel, complexity, complexityPct, complexityLabel };
}

/** "Story" — how the node evolved over milestones/commits (creation history). */
export function computeStory(node, relationships, edges) {
  if (!node) return null;
  const created = node.created_at ? new Date(node.created_at * 1000) : null;
  const modified = node.last_modified_at ? new Date(node.last_modified_at * 1000) : null;

  const relatedEdges = (edges ?? []).filter(e => {
    const src = edgeSource(e);
    const tgt = edgeTarget(e);
    return src === node.id || tgt === node.id;
  });

  const modCount = node.churn_count_30d ?? 0;
  const fieldCount = relationships.fields.length;

  const parts = [];
  if (created) parts.push(`Created ${created.toLocaleDateString()}`);
  if (node.created_sha) parts.push(`in commit ${node.created_sha.slice(0, 7)}`);
  if (node.last_modified_sha && node.last_modified_sha !== node.created_sha) parts.push(`last modified in ${node.last_modified_sha.slice(0, 7)}`);
  if (modified) parts.push(`modified ${timeAgo(modified)}`);
  if (fieldCount > 0) parts.push(`${fieldCount} field${fieldCount !== 1 ? 's' : ''}`);
  if (modCount > 0) parts.push(`${modCount} change${modCount !== 1 ? 's' : ''} in last 30 days`);
  if (relatedEdges.length > 0) parts.push(`${relatedEdges.length} relationship${relatedEdges.length !== 1 ? 's' : ''}`);

  return parts.length > 0 ? parts.join('. ') + '.' : null;
}

/** Extract a method's parameter signature from doc_comment, qualified_name,
 * or metadata (whichever carries one). */
export function extractMethodSignature(method) {
  let params = '';
  let returnType = '';

  if (method.doc_comment) {
    const sigMatch = method.doc_comment.match(/\(([^)]*)\)/);
    if (sigMatch) params = sigMatch[1].trim();
    const retMatch = method.doc_comment.match(/->\s*(.+?)(?:\s*$|\s*\n)/);
    if (retMatch) returnType = retMatch[1].trim();
  }

  if (!params && method.qualified_name) {
    const sigMatch = method.qualified_name.match(/\(([^)]*)\)/);
    if (sigMatch) params = sigMatch[1].trim();
  }

  if (method.metadata) {
    try {
      const meta = typeof method.metadata === 'string' ? JSON.parse(method.metadata) : method.metadata;
      if (meta.params && !params) params = meta.params;
      if (meta.return_type && !returnType) returnType = meta.return_type;
      if (meta.signature) {
        const sigMatch = meta.signature.match(/\(([^)]*)\)/);
        if (sigMatch && !params) params = sigMatch[1].trim();
        const retMatch = meta.signature.match(/->\s*(.+)/);
        if (retMatch && !returnType) returnType = retMatch[1].trim();
      }
    } catch { /* ignore */ }
  }

  return { params: params || null, returnType: returnType || null };
}

/** Extract the HTTP route path + method from an edge's JSON metadata
 * (`{"path": "/api/v1/x", "method": "POST"}` — the extractor convention,
 * see rust_extractor.rs extract_endpoints_from_text). */
export function edgeRouteInfo(e) {
  if (!e?.metadata) return null;
  try {
    const meta = typeof e.metadata === 'string' ? JSON.parse(e.metadata) : e.metadata;
    if (!meta || (!meta.path && !meta.method)) return null;
    return { path: meta.path ?? null, method: meta.method ?? null };
  } catch {
    return null;
  }
}

/** Endpoint route + handler info. The route path/method come from
 * RoutesTo/Contains edge metadata first (canonical extractor location),
 * then doc_comment (`"GET /path"`), then the node names. */
export function computeEndpointMeta(node, nodes, edges) {
  if (!node || node.node_type !== 'endpoint') return null;
  const routesTo = [];
  let method = '';
  let path = '';

  for (const e of edges) {
    const et = edgeType(e);
    if (et === 'routes_to' && edgeSource(e) === node.id) {
      const route = edgeRouteInfo(e);
      if (route?.path && !path) { path = route.path; method = route.method ?? ''; }
      const handler = nodes.find(n => n.id === edgeTarget(e));
      if (handler) routesTo.push(handler);
    }
    // Contains edges from a module to the endpoint also carry the route meta
    if (et === 'contains' && edgeTarget(e) === node.id && !path) {
      const route = edgeRouteInfo(e);
      if (route?.path) { path = route.path; method = route.method ?? ''; }
    }
  }

  if ((!method || !path) && node.doc_comment) {
    const parts = node.doc_comment.match(/^(GET|POST|PUT|DELETE|PATCH)\s+(.+)/);
    if (parts) { method = method || parts[1]; path = path || parts[2]; }
  }

  return { routesTo, method, path };
}

/** Endpoint flow: call chain from the handler, BFS depth 3 (the
 * "auth → ABAC → business logic → respond" flow diagram). */
export function computeEndpointFlow(node, nodes, edges) {
  if (!node || node.node_type !== 'endpoint') return [];
  const handlerIds = new Set();
  for (const e of edges) {
    if (edgeType(e) === 'routes_to' && edgeSource(e) === node.id) {
      handlerIds.add(edgeTarget(e));
    }
  }
  if (handlerIds.size === 0) return [];

  const visited = new Set();
  const chain = [];
  let frontier = [...handlerIds];
  for (const hid of frontier) {
    visited.add(hid);
    const handlerNode = nodes.find(n => n.id === hid);
    if (handlerNode) chain.push({ node: handlerNode, depth: 0 });
  }

  for (let depth = 1; depth <= 3; depth++) {
    const nextFrontier = [];
    for (const currentId of frontier) {
      for (const e of edges) {
        const src = edgeSource(e);
        const tgt = edgeTarget(e);
        const et = edgeType(e);
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
}

/** Count test functions that can reach this endpoint (directly or via its handler). */
export function computeEndpointTestCount(node, relationships, nodes, edges) {
  if (!node || node.node_type !== 'endpoint') return 0;
  const uniqueTests = new Set(relationships.testedBy.map(t => t.id));
  let count = uniqueTests.size;
  if (count > 0) return count;
  const handlerIds = new Set();
  for (const e of edges) {
    if (edgeType(e) === 'routes_to' && edgeSource(e) === node.id) {
      handlerIds.add(edgeTarget(e));
    }
  }
  const testNodes = nodes.filter(n => n.test_node);
  for (const tn of testNodes) {
    for (const e of edges) {
      const et = edgeType(e);
      if ((et === 'calls' || et === 'routes_to') && edgeSource(e) === tn.id && (handlerIds.has(edgeTarget(e)) || edgeTarget(e) === node.id)) {
        count++;
        break;
      }
    }
  }
  return count;
}

/** Extract request/response body shapes from doc_comment, edge metadata, or
 * handler call targets (request/response-named types). */
export function computeEndpointShapes(node, relationships, nodes, edges) {
  if (!node || node.node_type !== 'endpoint') return null;
  let requestBody = null;
  let responseBody = null;
  let params = [];

  if (node.doc_comment) {
    const reqMatch = node.doc_comment.match(/(?:request|body|input|accepts)\s*[:=]\s*(.+?)(?:\n|$)/i);
    if (reqMatch) requestBody = reqMatch[1].trim();
    const resMatch = node.doc_comment.match(/(?:response|returns|output|->)\s*[:=]?\s*(.+?)(?:\n|$)/i);
    if (resMatch) responseBody = resMatch[1].trim();
    const paramMatches = node.doc_comment.matchAll(/[:{}](\w+)/g);
    for (const m of paramMatches) {
      if (!params.includes(m[1])) params.push(m[1]);
    }
  }

  if (node.metadata) {
    try {
      const meta = typeof node.metadata === 'string' ? JSON.parse(node.metadata) : node.metadata;
      if (meta.request_body && !requestBody) requestBody = meta.request_body;
      if (meta.response_body && !responseBody) responseBody = meta.response_body;
      if (meta.params && params.length === 0) params = Array.isArray(meta.params) ? meta.params : [meta.params];
    } catch { /* ignore */ }
  }

  if (!requestBody || !responseBody) {
    for (const handler of relationships.routesTo ?? []) {
      for (const e of edges) {
        const src = edgeSource(e);
        const tgt = edgeTarget(e);
        const et = edgeType(e);
        if (src === handler.id && (et === 'calls' || et === 'field_of')) {
          const targetNode = nodes.find(n => n.id === tgt);
          if (targetNode && targetNode.node_type === 'type') {
            const name = targetNode.name.toLowerCase();
            if (!requestBody && (name.includes('request') || name.includes('input') || name.includes('body') || name.includes('payload'))) {
              requestBody = targetNode.qualified_name ?? targetNode.name;
            }
            if (!responseBody && (name.includes('response') || name.includes('output') || name.includes('result') || name.includes('reply'))) {
              responseBody = targetNode.qualified_name ?? targetNode.name;
            }
          }
        }
      }
    }
  }

  if (params.length === 0 && (node.qualified_name || node.name)) {
    const routePath = node.qualified_name || node.name;
    const pathParams = routePath.matchAll(/[:{](\w+)[}]?/g);
    for (const m of pathParams) {
      if (!params.includes(m[1])) params.push(m[1]);
    }
  }

  if (!requestBody && !responseBody && params.length === 0) return null;
  return { requestBody, responseBody, params };
}

/** Extract gate metadata (roles, auth requirements) from node metadata or doc_comment. */
export function computeEndpointGates(node) {
  if (!node || node.node_type !== 'endpoint') return [];
  const gates = [];
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
}

/** Type-annotation text for a field node. Every extractor stores the
 * field's TYPE here, not a doc string (rust_extractor.rs make_node with
 * `Some(type_str)`, go_extractor.rs / typescript_extractor.rs /
 * python_extractor.rs set `doc_comment = type annotation`). The
 * qualified_name's last segment is the FIELD NAME
 * (`Parent::field_name` / `Parent.field_name`), never the type.
 */
function fieldAnnotation(field) {
  const doc = field?.doc_comment?.trim();
  return doc || null;
}

const TYPE_WRAPPER_NAMES = new Set([
  'Option', 'Optional', 'Vec', 'Result', 'Box', 'Rc', 'Arc', 'RefCell', 'Cow',
  'HashMap', 'BTreeMap', 'HashSet', 'BTreeSet', 'Map', 'Array', 'List', 'Set', 'Promise',
]);

const PRIMITIVE_TYPE_NAMES = new Set([
  'String', 'str', 'char', 'bool', 'boolean', 'byte', 'rune', 'number', 'int',
  'float', 'i8', 'i16', 'i32', 'i64', 'i128', 'isize', 'u8', 'u16', 'u32',
  'u64', 'u128', 'usize', 'f32', 'f64', 'int8', 'int16', 'int32', 'int64',
  'uint', 'uint8', 'uint16', 'uint32', 'uint64', 'float32', 'float64',
  'complex64', 'complex128', 'any', 'unknown', 'never', 'void', 'object',
  'null', 'undefined', 'true', 'false', 'self', 'interface', 'type',
]);

/** Candidate user type identifiers in a type annotation, outermost first.
 * `Option<Id>` → ['Id'], `Vec<Task>` → ['Task'], `[]Bar` / `*Foo` → ['Bar'],
 * `TaskStatus | null` → ['TaskStatus']. Wrappers and language primitives
 * are skipped — they are never the navigable type.
 */
function annotationTypeIdentifiers(annotation) {
  const ids = annotation.match(/[A-Za-z_][A-Za-z0-9_]*/g) ?? [];
  return ids.filter((id) => !TYPE_WRAPPER_NAMES.has(id) && !PRIMITIVE_TYPE_NAMES.has(id));
}

function isTypeLike(n) {
  return n && (n.node_type === 'type' || n.node_type === 'interface' || n.node_type === 'trait');
}

function findTypeNodeByName(nodes, typeName) {
  return nodes.find((n) =>
    isTypeLike(n) &&
    (n.name === typeName ||
      n.qualified_name?.endsWith(`::${typeName}`) ||
      n.qualified_name?.endsWith(`.${typeName}`))
  ) ?? null;
}

/** Resolve a field's type for the clickable field-type link (spec §2:
 * "Every field type is clickable → navigates to that type").
 *
 * Primary source: the DependsOn edge every extractor emits from the field
 * node to the EXACT type node when the field's type is in the graph
 * (rust_extractor.rs / go_extractor.rs / typescript_extractor.rs /
 * python_extractor.rs). Fallback: match a type node by the identifiers in
 * the type annotation (doc_comment). Returns { typeNode, annotation } —
 * typeNode is null when the type is not in the graph (primitives, external
 * types) and the annotation renders as plain text.
 */
export function resolveFieldType(field, nodes, edges) {
  if (!field || !nodes) return null;
  const annotation = fieldAnnotation(field);

  // 1. DependsOn edge: the extractor already resolved the exact type node.
  for (const e of edges ?? []) {
    if (edgeType(e) !== 'depends_on') continue;
    if (edgeSource(e) !== field.id) continue;
    const target = nodes.find((n) => n.id === edgeTarget(e));
    if (isTypeLike(target)) {
      return { typeNode: target, annotation: annotation ?? target.name };
    }
  }

  // 2. Match a type node by the annotation's identifiers.
  if (annotation) {
    for (const typeName of annotationTypeIdentifiers(annotation)) {
      const typeNode = findTypeNodeByName(nodes, typeName);
      if (typeNode) return { typeNode, annotation };
    }
    return { typeNode: null, annotation };
  }

  return null;
}

/** Resolve the spec path for a spec node (extractor convention: spec nodes
 * carry the path as name/qualified_name/spec_path/file_path). */
export function specPathOf(node) {
  if (!node) return null;
  if (node.spec_path) return node.spec_path;
  const candidates = [node.name, node.qualified_name, node.file_path];
  for (const c of candidates) {
    if (typeof c === 'string' && c.endsWith('.md')) return c;
  }
  return null;
}

/** All nodes governed by this spec: matching spec_path fields plus
 * GovernedBy edges pointing at the spec node. Soft-deleted nodes are
 * included (marked unrealized by computeSpecCompleteness). */
export function computeSpecGovernedNodes(node, nodes, edges) {
  if (!node || node.node_type !== 'spec') return [];
  const specId = node.id;
  const specPath = specPathOf(node);
  const governed = new Map();

  for (const n of nodes) {
    if (n.id === specId) continue;
    if (specPath && n.spec_path && (n.spec_path === specPath || n.spec_path === node.name || n.spec_path === node.qualified_name)) {
      governed.set(n.id, n);
    }
  }

  for (const e of edges) {
    const et = edgeType(e);
    if (et === 'governed_by' && edgeTarget(e) === specId) {
      const srcNode = nodes.find(n => n.id === edgeSource(e));
      if (srcNode) governed.set(srcNode.id, srcNode);
    }
  }

  return [...governed.values()];
}

/** Implementation completeness for a spec: realized = non-deleted governed
 * nodes (present in current extraction), tested = realized nodes with test
 * evidence (coverage > 0, test node, or a test caller). */
export function computeSpecCompleteness(governedNodes, nodes, edges) {
  const total = governedNodes.length;
  if (total === 0) return { total: 0, realized: 0, tested: 0, pct: 0 };

  const realizedNodes = governedNodes.filter(n => !n.deleted_at);
  let tested = 0;
  for (const gn of realizedNodes) {
    if (gn.test_coverage != null && gn.test_coverage > 0) {
      tested++;
    } else if (gn.test_node) {
      tested++;
    } else {
      for (const e of edges) {
        const et = edgeType(e);
        if ((et === 'tests' || et === 'calls') && edgeTarget(e) === gn.id) {
          const srcNode = nodes.find(n => n.id === edgeSource(e));
          if (srcNode?.test_node) { tested++; break; }
        }
      }
    }
  }

  const realized = realizedNodes.length;
  const pct = total > 0 ? Math.round((realized / total) * 100) : 0;
  return { total, realized, tested, pct };
}
