/**
 * Deterministic mock graph data for visual regression tests.
 *
 * This fixture provides a realistic graph with packages, modules, types,
 * functions, endpoints, and edges. The data is designed to exercise:
 * - Semantic zoom at multiple levels (packages → modules → types/functions)
 * - Filter presets (endpoints, types, calls, dependencies)
 * - View query rendering (groups, callouts, narrative)
 * - Blast radius interactive mode (tiered BFS coloring)
 *
 * Node IDs are deterministic strings so layout is reproducible.
 */

const REPO_ID = 'seed-repo-1';
const NOW = 1711324800; // Fixed epoch for deterministic timestamps

/**
 * Create a graph node with sensible defaults.
 */
function node(id, name, nodeType, filePath, opts = {}) {
  return {
    id,
    repo_id: REPO_ID,
    node_type: nodeType,
    name,
    qualified_name: opts.qualified_name ?? `${filePath.replace(/\//g, '.')}::${name}`,
    file_path: filePath,
    line_start: opts.line_start ?? 1,
    line_end: opts.line_end ?? 50,
    visibility: opts.visibility ?? 'public',
    doc_comment: opts.doc_comment ?? null,
    spec_path: opts.spec_path ?? null,
    spec_confidence: opts.spec_confidence ?? 'none',
    last_modified_sha: 'abc123',
    last_modified_by: null,
    last_modified_at: NOW - 3600,
    created_sha: 'abc100',
    created_at: NOW - 86400,
    complexity: opts.complexity ?? 5,
    churn_count_30d: opts.churn_count_30d ?? 0,
    test_coverage: opts.test_coverage ?? null,
    first_seen_at: NOW - 86400,
    last_seen_at: NOW,
    deleted_at: null,
    test_node: opts.test_node ?? false,
  };
}

/**
 * Create a graph edge.
 */
function edge(id, sourceId, targetId, edgeType) {
  return {
    id,
    repo_id: REPO_ID,
    source_id: sourceId,
    target_id: targetId,
    edge_type: edgeType,
    metadata: null,
    first_seen_at: NOW - 86400,
    last_seen_at: NOW,
    deleted_at: null,
  };
}

// ── Packages ─────────────────────────────────────────────────────────────────
const packages = [
  node('pkg-api', 'api', 'package', 'src/api'),
  node('pkg-domain', 'domain', 'package', 'src/domain'),
  node('pkg-infra', 'infra', 'package', 'src/infra'),
];

// ── Modules ──────────────────────────────────────────────────────────────────
const modules = [
  node('mod-api-handlers', 'handlers', 'module', 'src/api/handlers'),
  node('mod-api-middleware', 'middleware', 'module', 'src/api/middleware'),
  node('mod-domain-models', 'models', 'module', 'src/domain/models'),
  node('mod-domain-services', 'services', 'module', 'src/domain/services'),
  node('mod-infra-db', 'db', 'module', 'src/infra/db'),
  node('mod-infra-cache', 'cache', 'module', 'src/infra/cache'),
];

// ── Types ────────────────────────────────────────────────────────────────────
const types = [
  node('type-user', 'User', 'type', 'src/domain/models/user.rs', {
    spec_path: 'specs/system/platform-model.md',
    spec_confidence: 'high',
    complexity: 8,
  }),
  node('type-repo', 'Repository', 'type', 'src/domain/models/repo.rs', {
    spec_path: 'specs/system/platform-model.md',
    spec_confidence: 'high',
    complexity: 12,
  }),
  node('type-task', 'Task', 'type', 'src/domain/models/task.rs', {
    spec_path: 'specs/system/platform-model.md',
    spec_confidence: 'medium',
    complexity: 15,
  }),
  node('type-agent', 'Agent', 'type', 'src/domain/models/agent.rs', {
    spec_path: 'specs/system/agent-runtime.md',
    spec_confidence: 'high',
    complexity: 20,
  }),
  node('type-workspace', 'Workspace', 'type', 'src/domain/models/workspace.rs', {
    spec_path: 'specs/system/platform-model.md',
    spec_confidence: 'high',
    complexity: 6,
  }),
  node('type-db-pool', 'DbPool', 'type', 'src/infra/db/pool.rs', {
    complexity: 3,
  }),
  node('type-cache-entry', 'CacheEntry', 'type', 'src/infra/cache/entry.rs', {
    complexity: 4,
  }),
];

// ── Traits / Interfaces ─────────────────────────────────────────────────────
const traits = [
  node('trait-repo-port', 'RepositoryPort', 'trait', 'src/domain/services/ports.rs', {
    spec_path: 'specs/development/architecture.md',
    spec_confidence: 'high',
    complexity: 10,
  }),
  node('trait-cache-port', 'CachePort', 'trait', 'src/domain/services/ports.rs', {
    complexity: 6,
  }),
];

// ── Functions ────────────────────────────────────────────────────────────────
const functions = [
  node('fn-create-user', 'create_user', 'function', 'src/domain/services/user_service.rs', {
    complexity: 12,
    churn_count_30d: 5,
  }),
  node('fn-find-user', 'find_user', 'function', 'src/domain/services/user_service.rs', {
    complexity: 4,
  }),
  node('fn-create-task', 'create_task', 'function', 'src/domain/services/task_service.rs', {
    complexity: 18,
    churn_count_30d: 8,
  }),
  node('fn-spawn-agent', 'spawn_agent', 'function', 'src/domain/services/agent_service.rs', {
    spec_path: 'specs/system/agent-runtime.md',
    spec_confidence: 'high',
    complexity: 25,
    churn_count_30d: 3,
  }),
  node('fn-auth-check', 'auth_check', 'function', 'src/api/middleware/auth.rs', {
    complexity: 14,
    churn_count_30d: 2,
  }),
  node('fn-validate-input', 'validate_input', 'function', 'src/api/middleware/validation.rs', {
    complexity: 8,
  }),
  node('fn-db-connect', 'connect', 'function', 'src/infra/db/pool.rs', {
    complexity: 7,
  }),
  node('fn-cache-get', 'cache_get', 'function', 'src/infra/cache/client.rs', {
    complexity: 3,
  }),
  node('fn-cache-set', 'cache_set', 'function', 'src/infra/cache/client.rs', {
    complexity: 3,
  }),
];

// ── Endpoints ────────────────────────────────────────────────────────────────
const endpoints = [
  node('ep-post-users', 'POST /users', 'endpoint', 'src/api/handlers/users.rs', {
    spec_path: 'specs/system/platform-model.md',
    spec_confidence: 'high',
    complexity: 10,
  }),
  node('ep-get-users', 'GET /users/:id', 'endpoint', 'src/api/handlers/users.rs', {
    spec_path: 'specs/system/platform-model.md',
    spec_confidence: 'high',
    complexity: 6,
  }),
  node('ep-post-tasks', 'POST /tasks', 'endpoint', 'src/api/handlers/tasks.rs', {
    spec_path: 'specs/system/platform-model.md',
    spec_confidence: 'medium',
    complexity: 14,
  }),
  node('ep-post-agents', 'POST /agents/spawn', 'endpoint', 'src/api/handlers/agents.rs', {
    spec_path: 'specs/system/agent-runtime.md',
    spec_confidence: 'high',
    complexity: 16,
  }),
  node('ep-get-repos', 'GET /repos', 'endpoint', 'src/api/handlers/repos.rs', {
    complexity: 5,
  }),
];

// ── Test nodes ───────────────────────────────────────────────────────────────
const testNodes = [
  node('test-user-service', 'test_create_user', 'function', 'tests/user_service_test.rs', {
    test_node: true,
    complexity: 6,
  }),
  node('test-task-service', 'test_create_task', 'function', 'tests/task_service_test.rs', {
    test_node: true,
    complexity: 4,
  }),
];

// ── All nodes ────────────────────────────────────────────────────────────────
const allNodes = [
  ...packages,
  ...modules,
  ...types,
  ...traits,
  ...functions,
  ...endpoints,
  ...testNodes,
];

// ── Edges ────────────────────────────────────────────────────────────────────
let edgeId = 0;
const nextEdgeId = () => `edge-${++edgeId}`;

const allEdges = [
  // Contains: packages → modules
  edge(nextEdgeId(), 'pkg-api', 'mod-api-handlers', 'contains'),
  edge(nextEdgeId(), 'pkg-api', 'mod-api-middleware', 'contains'),
  edge(nextEdgeId(), 'pkg-domain', 'mod-domain-models', 'contains'),
  edge(nextEdgeId(), 'pkg-domain', 'mod-domain-services', 'contains'),
  edge(nextEdgeId(), 'pkg-infra', 'mod-infra-db', 'contains'),
  edge(nextEdgeId(), 'pkg-infra', 'mod-infra-cache', 'contains'),

  // Contains: modules → types
  edge(nextEdgeId(), 'mod-domain-models', 'type-user', 'contains'),
  edge(nextEdgeId(), 'mod-domain-models', 'type-repo', 'contains'),
  edge(nextEdgeId(), 'mod-domain-models', 'type-task', 'contains'),
  edge(nextEdgeId(), 'mod-domain-models', 'type-agent', 'contains'),
  edge(nextEdgeId(), 'mod-domain-models', 'type-workspace', 'contains'),
  edge(nextEdgeId(), 'mod-infra-db', 'type-db-pool', 'contains'),
  edge(nextEdgeId(), 'mod-infra-cache', 'type-cache-entry', 'contains'),
  edge(nextEdgeId(), 'mod-domain-services', 'trait-repo-port', 'contains'),
  edge(nextEdgeId(), 'mod-domain-services', 'trait-cache-port', 'contains'),

  // Contains: modules → functions
  edge(nextEdgeId(), 'mod-domain-services', 'fn-create-user', 'contains'),
  edge(nextEdgeId(), 'mod-domain-services', 'fn-find-user', 'contains'),
  edge(nextEdgeId(), 'mod-domain-services', 'fn-create-task', 'contains'),
  edge(nextEdgeId(), 'mod-domain-services', 'fn-spawn-agent', 'contains'),
  edge(nextEdgeId(), 'mod-api-middleware', 'fn-auth-check', 'contains'),
  edge(nextEdgeId(), 'mod-api-middleware', 'fn-validate-input', 'contains'),
  edge(nextEdgeId(), 'mod-infra-db', 'fn-db-connect', 'contains'),
  edge(nextEdgeId(), 'mod-infra-cache', 'fn-cache-get', 'contains'),
  edge(nextEdgeId(), 'mod-infra-cache', 'fn-cache-set', 'contains'),

  // Contains: modules → endpoints
  edge(nextEdgeId(), 'mod-api-handlers', 'ep-post-users', 'contains'),
  edge(nextEdgeId(), 'mod-api-handlers', 'ep-get-users', 'contains'),
  edge(nextEdgeId(), 'mod-api-handlers', 'ep-post-tasks', 'contains'),
  edge(nextEdgeId(), 'mod-api-handlers', 'ep-post-agents', 'contains'),
  edge(nextEdgeId(), 'mod-api-handlers', 'ep-get-repos', 'contains'),

  // Calls: endpoints → functions
  edge(nextEdgeId(), 'ep-post-users', 'fn-create-user', 'calls'),
  edge(nextEdgeId(), 'ep-get-users', 'fn-find-user', 'calls'),
  edge(nextEdgeId(), 'ep-post-tasks', 'fn-create-task', 'calls'),
  edge(nextEdgeId(), 'ep-post-agents', 'fn-spawn-agent', 'calls'),

  // Calls: endpoints → middleware
  edge(nextEdgeId(), 'ep-post-users', 'fn-auth-check', 'calls'),
  edge(nextEdgeId(), 'ep-get-users', 'fn-auth-check', 'calls'),
  edge(nextEdgeId(), 'ep-post-tasks', 'fn-auth-check', 'calls'),
  edge(nextEdgeId(), 'ep-post-agents', 'fn-auth-check', 'calls'),
  edge(nextEdgeId(), 'ep-post-users', 'fn-validate-input', 'calls'),
  edge(nextEdgeId(), 'ep-post-tasks', 'fn-validate-input', 'calls'),

  // Calls: functions → infra
  edge(nextEdgeId(), 'fn-create-user', 'fn-db-connect', 'calls'),
  edge(nextEdgeId(), 'fn-find-user', 'fn-db-connect', 'calls'),
  edge(nextEdgeId(), 'fn-find-user', 'fn-cache-get', 'calls'),
  edge(nextEdgeId(), 'fn-create-task', 'fn-db-connect', 'calls'),
  edge(nextEdgeId(), 'fn-spawn-agent', 'fn-db-connect', 'calls'),

  // Implements: infra implements ports
  edge(nextEdgeId(), 'type-db-pool', 'trait-repo-port', 'implements'),
  edge(nextEdgeId(), 'type-cache-entry', 'trait-cache-port', 'implements'),

  // DependsOn: domain depends on ports
  edge(nextEdgeId(), 'fn-create-user', 'trait-repo-port', 'depends_on'),
  edge(nextEdgeId(), 'fn-find-user', 'trait-repo-port', 'depends_on'),
  edge(nextEdgeId(), 'fn-create-task', 'trait-repo-port', 'depends_on'),
  edge(nextEdgeId(), 'fn-find-user', 'trait-cache-port', 'depends_on'),

  // RoutesTo: endpoints route to handlers
  edge(nextEdgeId(), 'ep-post-users', 'fn-create-user', 'routes_to'),
  edge(nextEdgeId(), 'ep-get-users', 'fn-find-user', 'routes_to'),

  // GovernedBy: functions governed by specs
  edge(nextEdgeId(), 'fn-spawn-agent', 'type-agent', 'governed_by'),

  // Test calls
  edge(nextEdgeId(), 'test-user-service', 'fn-create-user', 'calls'),
  edge(nextEdgeId(), 'test-task-service', 'fn-create-task', 'calls'),
];

/**
 * The complete mock graph response matching KnowledgeGraphResponse.
 */
export const MOCK_GRAPH = {
  repo_id: REPO_ID,
  nodes: allNodes,
  edges: allEdges,
};

/**
 * A view query with groups, callouts, and narrative markers for testing
 * view query rendering.
 */
export const VIEW_QUERY_WITH_ANNOTATIONS = {
  scope: { type: 'all' },
  emphasis: {
    highlight: { matched: { color: '#3b82f6', label: 'Architecture' } },
    dim_unmatched: 0.15,
  },
  zoom: 'fit',
  annotation: {
    title: 'Architecture Overview',
    description: 'Request flow through the system',
  },
  groups: [
    { name: 'API Layer', nodes: ['ep-post-users', 'ep-get-users', 'ep-post-tasks', 'ep-post-agents', 'ep-get-repos'], color: '#3b82f6', label: 'API' },
    { name: 'Domain Layer', nodes: ['fn-create-user', 'fn-find-user', 'fn-create-task', 'fn-spawn-agent'], color: '#10b981', label: 'Domain' },
    { name: 'Infrastructure', nodes: ['fn-db-connect', 'fn-cache-get', 'fn-cache-set'], color: '#f59e0b', label: 'Infra' },
  ],
  callouts: [
    { node: 'fn-spawn-agent', text: 'Critical: agent lifecycle entry point', color: '#ef4444' },
    { node: 'fn-auth-check', text: 'Auth middleware — every request passes through', color: '#8b5cf6' },
  ],
  narrative: [
    { node: 'ep-post-users', text: '1. HTTP request arrives', order: 1 },
    { node: 'fn-auth-check', text: '2. Auth middleware validates token', order: 2 },
    { node: 'fn-create-user', text: '3. Domain service processes request', order: 3 },
    { node: 'fn-db-connect', text: '4. Persistence layer stores result', order: 4 },
  ],
};

/**
 * Blast radius view query for fn-spawn-agent (high connectivity node).
 */
export const BLAST_RADIUS_QUERY = {
  scope: {
    type: 'focus',
    node: 'fn-spawn-agent',
    edges: ['calls', 'implements', 'field_of', 'depends_on'],
    direction: 'incoming',
    depth: 10,
  },
  emphasis: {
    tiered_colors: ['#ef4444', '#f97316', '#eab308', '#94a3b8'],
    dim_unmatched: 0.12,
  },
  edges: { filter: ['calls', 'implements', 'field_of', 'depends_on'] },
  zoom: 'fit',
  annotation: {
    title: 'Blast radius: spawn_agent',
    description: '{{count}} transitive callers/implementors',
  },
};
